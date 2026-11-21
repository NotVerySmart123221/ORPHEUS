//! Worker event loop, UDP I/O polling, and tick scheduling.

use super::*;

pub(super) const GAME_SOCKET: Token = Token(0);
pub(super) const REQUESTS: Token = Token(1);
pub(super) const RECEIVE_BATCH: usize = 64;
pub(super) const INPUT_PER_PEER: usize = 16;
pub(super) const INPUT_PER_TURN: usize = 64;

pub(super) fn leave_message() -> Message {
    Message {
        id: 20304,
        fields: Fields::from([(0x78, Value::Unsigned(0)), (0x79, Value::Bool(false))]),
    }
}

pub(super) fn wall_nanoseconds() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(now.as_nanos()).unwrap_or(u64::MAX)
}

pub(super) fn send(socket: &UdpSocket, outgoing: Outgoing) -> io::Result<u32> {
    let count = socket.send_to(&outgoing.datagram, outgoing.address)?;
    if count != outgoing.datagram.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "partial game datagram",
        ));
    }
    Ok(outgoing.sequence)
}

pub(super) struct WorkerOptions {
    pub(super) collect_timing: bool,
    #[cfg(any(test, feature = "inspector"))]
    pub(super) debug_control: bool,
}

pub(super) fn run(
    socket: UdpSocket,
    mut poll: Poll,
    requests: mpsc::Receiver<Request>,
    stopping: Arc<AtomicBool>,
    schemas: Arc<Schemas>,
    ready: mpsc::SyncSender<io::Result<()>>,
    options: WorkerOptions,
) -> io::Result<()> {
    let patrol = match PatrolScript::load() {
        Ok(patrol) => patrol,
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(());
        }
    };
    let mut state = State::new(schemas, patrol);
    state.timing = options.collect_timing.then(WorkerTiming::new);
    #[cfg(any(test, feature = "inspector"))]
    {
        state.debug_control_enabled = options.debug_control;
    }
    #[cfg(feature = "inspector")]
    {
        crate::owner_trace::enabled();
        crate::owner_trace::start_playtest_capture();
    }
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    let start = Instant::now();
    let mut due = Duration::ZERO;
    let mut events = Events::with_capacity(2);
    // An oversized datagram must never be accepted as a truncated valid one.
    let mut buffer = [0; 65_536];
    let mut socket_ready = false;
    while !stopping.load(Ordering::Acquire) {
        let measure_turn = state.timing.is_some() && state.world_started();
        let request_start = measure_turn.then(Instant::now);
        let mut handled = 0;
        for request in requests.try_iter().take(RECEIVE_BATCH) {
            state.handle_request(request, start.elapsed())?;
            handled += 1;
        }
        let requests_work = request_start
            .filter(|_| handled != 0)
            .map(|at| at.elapsed());
        let receive_start = (measure_turn && socket_ready).then(Instant::now);
        if socket_ready {
            for _ in 0..RECEIVE_BATCH {
                match socket.recv_from(&mut buffer) {
                    Ok((count, address)) => {
                        state.receive(&socket, &buffer[..count], address, start.elapsed())?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        socket_ready = false;
                        break;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionRefused
                        ) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let receive_work = receive_start.map(|at| at.elapsed());
        let input_before = state.timing.as_ref().map(|timing| timing.input_budget.used);
        let input_start = measure_turn.then(Instant::now);
        let input_ready = state.advance_input(start.elapsed())?;
        if let (Some(timing), Some(at)) = (&mut state.timing, input_start) {
            let input_work = (input_before != Some(timing.input_budget.used)).then(|| at.elapsed());
            timing.record_turn_work(requests_work, receive_work, input_work);
        }
        let now = start.elapsed();
        if now >= due {
            let scheduled = due;
            let (next, missed) = next_tick_after(scheduled, now)
                .ok_or_else(|| io::Error::other("game tick clock exhausted"))?;
            due = next;
            let sample = state.timing.is_some() && !state.clock.draining() && state.world_started();
            let began = sample.then(Instant::now);
            state.tick(&socket, now, missed)?;
            if let (Some(timing), Some(began)) = (&mut state.timing, began) {
                timing.record_tick(began.elapsed(), now - scheduled, missed);
            }
        }
        let timeout = if socket_ready || input_ready || handled == RECEIVE_BATCH {
            Duration::ZERO
        } else {
            due.saturating_sub(start.elapsed())
        };
        match poll.poll(&mut events, Some(timeout)) {
            Ok(()) => {
                socket_ready |= events.iter().any(|event| event.token() == GAME_SOCKET);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
        if measure_turn && !timeout.is_zero() {
            if let Some(timing) = &mut state.timing {
                timing.record_poll_lateness(start.elapsed().saturating_sub(due));
            }
        }
    }
    Ok(())
}
