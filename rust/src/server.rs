//! Loopback-only native server for the verified retail Practice Range path.

use std::{
    collections::HashMap,
    fs, io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    accounts::{Account, Accounts, load_or_create_profile},
    bnet_protocol::Protocol,
    bnet_rpc::Player,
    bnet_service::{self, BnetService},
    catalog::ItemDb,
    collection::Collection,
    game_content::{GameMap, JUNKENSTEIN_MAP, MapCatalog},
    game_service::{AiPolicyStatus, AiReloadReport, GameService},
    jam_codec::{Fields, Schemas, Value},
    lobby_arcade::Arcade,
    lobby_celebrations::Celebrations,
    lobby_chat::{self, ChatRequest},
    lobby_gallery::EquipRequest,
    lobby_login::{Identity, SIGNED_IN_ELSEWHERE},
    lobby_lootbox::{LootBoxEngine, OpenBoxRequest, PurchaseRequest},
    lobby_menu::{MenuHero, extra_preload_themes},
    lobby_mode_rules::{CareerProfileRequest, owl_live_reply, player_names_reply},
    lobby_player_messages::{self as player, SettingsRequest},
    lobby_practice::{self, SoloPracticeHandoff},
    lobby_presence::{PRO_ACCOUNT_BITS, Presence},
    lobby_ranked::{LeaderboardPlaces, Ranked},
    lobby_session::{LobbySession, LoginContent, LoginRequest, SoloLoginParty},
    lobby_transport::{LobbyMessage, LobbyTransport},
    match_handoff::{
        LobbySeat, general_channel, join_chat_channel, leave_chat_channel, match_channel,
    },
    retail_templates::RetailTemplates,
    runtime::{BNET_DESCRIPTORS, check_practice_runtime},
    scene_assets::{PreparedScene, SceneAssets},
    service_tcp::{PeerStream, PeerWorkers, accept},
};

#[cfg(feature = "inspector")]
use crate::debug_http::DebugHttp;

const MAX_LOBBY_PEERS: usize = 32;
const ACCEPT_POLL: Duration = Duration::from_millis(10);
const CHAT_QUEUE_LIMIT: usize = 64;
const CHAT_FEATURE_RETRIES: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(10)];

pub struct ServerConfig {
    pub runtime: PathBuf,
    pub data: PathBuf,
    pub account: Option<String>,
    pub lobby_port: u16,
    pub game_port: u16,
    pub stall_port: u16,
    pub rpc_port: u16,
    pub debug_port: Option<u16>,
    pub debug_control: bool,
    pub junkenstein_validation: Option<PathBuf>,
    pub ai_script: Option<PathBuf>,
}

impl ServerConfig {
    pub fn new(runtime: PathBuf, data: PathBuf) -> Self {
        Self {
            runtime,
            data,
            account: None,
            lobby_port: 3724,
            game_port: 3730,
            stall_port: bnet_service::STALL_PORT,
            rpc_port: bnet_service::RPC_PORT,
            debug_port: None,
            debug_control: false,
            junkenstein_validation: None,
            ai_script: None,
        }
    }
}

pub struct Server {
    lobby_addr: SocketAddrV4,
    game_addr: SocketAddr,
    stop_lobby: mpsc::Sender<()>,
    lobby: Option<JoinHandle<io::Result<()>>>,
    bnet: BnetService,
    game: Option<Arc<GameService>>,
    ai_script: Option<PathBuf>,
    #[cfg(feature = "inspector")]
    debug: Option<DebugHttp>,
}

impl Server {
    /// Validate the data before starting the loopback listeners. This entry
    /// point serves the base graph bundle and contains no client launcher.
    pub fn start(config: ServerConfig) -> io::Result<Self> {
        #[cfg(not(feature = "lua-debug"))]
        if config.ai_script.is_some() {
            return Err(crate::game_service::lua_debug_required());
        }
        if config.debug_port.is_some() && !cfg!(feature = "inspector") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "--debug-port requires a build with --features inspector",
            ));
        }
        if config.debug_control && !cfg!(feature = "inspector") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "--debug-control requires a build with --features inspector",
            ));
        }
        if config.debug_control && config.debug_port.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--debug-control requires --debug-port",
            ));
        }
        check_practice_runtime(&config.runtime)?;
        let data = config.runtime.join("data");
        let signing_key = fs::read(data.join("lobby_state_blob.key"))?
            .try_into()
            .map_err(|_| invalid("lobby signing input must be 64 bytes"))?;
        let schemas = Schemas::load(data.join("schemas_174.json")).map_err(invalid)?;
        let game_schemas =
            Arc::new(Schemas::load(data.join("game_schemas_174.json")).map_err(invalid)?);
        let templates = RetailTemplates::load(data.join("retail_templates.json"))?;
        let items = Arc::new(ItemDb::load(&data)?);
        let collection = Collection::new(&templates, Arc::clone(&items))?;
        let presence = Presence::new(&templates)?;
        let ranked = Ranked::new(&templates)?;
        let arcade = Arcade::load(data.join("arcade_cards_174.json"), &templates)?;
        let celebrations = Celebrations::load(&data, &templates)?;
        let lootbox = LootBoxEngine::new(&collection);
        let extra_skins = extra_preload_themes(&items).collect();
        let mut assets = SceneAssets::load(&config.runtime)?;
        if let Some(path) = &config.junkenstein_validation {
            assets.set_junkenstein_validation(
                crate::junkenstein_validation::JunkensteinValidation::load(path)?,
            );
        }
        if config.debug_port.is_some() {
            assets.enable_contact_trace();
        }
        let maps = MapCatalog::load(data.join("game_maps_174.json"))?;
        let scene = assets.prepare(GameMap::practice_range())?;
        if !scene.movement_ready() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the native Practice Range requires the base graph bundle",
            ));
        }
        let protocol =
            Arc::new(Protocol::load(config.runtime.join(BNET_DESCRIPTORS)).map_err(invalid)?);
        let template = config.data.join("profile.json");
        let accounts = Accounts::new(config.data.join("profiles"), &template)?;
        let local = local_account(
            &accounts,
            &config.data,
            &template,
            config.account.as_deref(),
        )?;
        let listener = TcpListener::bind(addr(config.lobby_port))?;
        listener.set_nonblocking(true)?;
        let SocketAddr::V4(lobby_addr) = listener.local_addr()? else {
            unreachable!("IPv4 lobby bind")
        };
        #[cfg(feature = "inspector")]
        let game = Arc::new(GameService::bind_with_options(
            addr(config.game_port).into(),
            game_schemas,
            config.debug_port.is_some(),
            config.debug_control,
        )?);
        #[cfg(not(feature = "inspector"))]
        let game = Arc::new(GameService::bind_with_timing(
            addr(config.game_port).into(),
            game_schemas,
            config.debug_port.is_some(),
        )?);
        if let Some(path) = &config.ai_script {
            let report = game.reload_ai_script_file(path)?;
            eprintln!(
                "[ai] loaded {} (abi v{}, gen {}, sha256 {})",
                path.display(),
                report.abi_version,
                report.generation,
                report.digest
            );
        }
        let game_addr = game.local_addr();
        #[cfg(feature = "inspector")]
        let debug = if let Some(port) = config.debug_port {
            const MAX_LINK_DIAGNOSTICS: usize = 16;
            for (index, link) in assets
                .graphs()
                .dangling_links()
                .take(MAX_LINK_DIAGNOSTICS + 1)
                .enumerate()
            {
                if index == MAX_LINK_DIAGNOSTICS {
                    eprintln!("[script] additional dangling links omitted");
                    break;
                }
                let plug: String = link.plug.chars().take(128).collect();
                eprintln!(
                    "[script] graph {} node {} plug {:?} link {} points to absent node {}",
                    link.graph, link.source, plug, link.link_index, link.target
                );
            }
            let graphs = assets.graphs();
            let (client_only, plain_states) = graphs
                .graph_indexes()
                .filter_map(|index| graphs.graph(index))
                .flat_map(|graph| &graph.nodes)
                .fold((0, 0), |(client_only, plain_states), node| {
                    (
                        client_only + usize::from(node.client_only),
                        plain_states + usize::from(node.declares_plain_state()),
                    )
                });
            eprintln!("[script] {client_only} nodes declared client-only");
            eprintln!("[script] {plain_states} nodes declared as authored plain states");
            let geometry = scene.debug_geometry();
            let worker = Arc::clone(&game);
            if config.debug_control {
                let control_worker = Arc::clone(&game);
                Some(DebugHttp::bind_with_control(
                    port,
                    geometry,
                    move || worker.debug_snapshot(),
                    move |id, command| control_worker.debug_room_control(id, command),
                )?)
            } else {
                Some(DebugHttp::bind(port, geometry, move || {
                    worker.debug_snapshot()
                })?)
            }
        } else {
            None
        };
        let bnet_account = Arc::clone(&local);
        let bnet = BnetService::start(
            addr(config.stall_port),
            addr(config.rpc_port),
            lobby_addr,
            protocol,
            move |_| {
                let account = u64::from(bnet_account.account_lo);
                Player {
                    account,
                    game_account: account ^ PRO_ACCOUNT_BITS,
                    battle_tag: bnet_account.battle_tag(),
                    name: bnet_account.name.clone(),
                }
            },
        )?;
        let shared = Arc::new(Lobby {
            signing_key,
            schemas,
            templates,
            collection,
            lootbox,
            presence,
            ranked,
            arcade,
            celebrations,
            extra_skins,
            assets,
            maps,
            first_scene: Mutex::new(Some(scene)),
            accounts,
            local,
            active: Mutex::new(HashMap::new()),
            game: Arc::clone(&game),
        });
        let (stop_lobby, shutdown) = mpsc::channel();
        let lobby = thread::Builder::new()
            .name("orpheus-lobby".into())
            .spawn(move || serve(listener, shared, shutdown))?;
        Ok(Self {
            lobby_addr,
            game_addr,
            stop_lobby,
            lobby: Some(lobby),
            bnet,
            game: Some(game),
            ai_script: config.ai_script,
            #[cfg(feature = "inspector")]
            debug,
        })
    }

    pub fn lobby_addr(&self) -> SocketAddrV4 {
        self.lobby_addr
    }
    pub fn game_addr(&self) -> SocketAddr {
        self.game_addr
    }
    pub fn stall_addr(&self) -> SocketAddr {
        self.bnet.stall_addr()
    }
    pub fn rpc_addr(&self) -> SocketAddr {
        self.bnet.rpc_addr()
    }

    pub fn debug_addr(&self) -> Option<SocketAddr> {
        #[cfg(feature = "inspector")]
        {
            self.debug.as_ref().map(DebugHttp::addr)
        }
        #[cfg(not(feature = "inspector"))]
        {
            None
        }
    }

    pub fn reload_ai_script(&self) -> io::Result<AiReloadReport> {
        let Some(path) = &self.ai_script else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no AI script path configured (pass --ai-script <lua-file> or call reload_ai_script_file)",
            ));
        };
        self.reload_ai_script_file(path)
    }

    pub fn reload_ai_script_file(&self, path: &Path) -> io::Result<AiReloadReport> {
        let Some(game) = &self.game else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "game service is not running",
            ));
        };
        game.reload_ai_script_file(path)
    }

    pub fn ai_policy_status(&self) -> io::Result<AiPolicyStatus> {
        let Some(game) = &self.game else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "game service is not running",
            ));
        };
        game.ai_policy_status()
    }

    pub fn stop(&mut self) -> io::Result<()> {
        #[cfg(feature = "inspector")]
        let debug = self.debug.take().map_or(Ok(()), |mut debug| debug.stop());
        #[cfg(not(feature = "inspector"))]
        let debug = Ok(());
        let bnet = self.bnet.stop();
        let lobby = if let Some(worker) = self.lobby.take() {
            let _ = self.stop_lobby.send(());
            worker
                .join()
                .map_err(|_| io::Error::other("lobby listener panicked"))
                .and_then(|result| result)
        } else {
            Ok(())
        };
        let game = self.game.take().map_or(Ok(()), |game| {
            Arc::try_unwrap(game)
                .map_err(|_| io::Error::other("game worker still in use"))?
                .stop()
        });
        debug.and(bnet).and(lobby).and(game)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn local_account(
    accounts: &Accounts,
    data: &Path,
    template: &Path,
    chosen: Option<&str>,
) -> io::Result<Arc<Account>> {
    if let Some(name) = chosen.map(str::trim) {
        if name.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty local account name",
            ));
        }
        return accounts.get(name);
    }
    let saved = accounts.all_saved()?;
    let default = match fs::read_to_string(data.join("default_account.txt")) {
        Ok(name) => name.trim().to_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if let Some(name) = saved
        .iter()
        .find(|name| name.eq_ignore_ascii_case(&default))
        .or(saved.first())
    {
        return accounts.get(name);
    }
    accounts.get(&load_or_create_profile(template)?.player_name)
}

struct Lobby {
    signing_key: [u8; 64],
    schemas: Schemas,
    templates: RetailTemplates,
    collection: Collection,
    lootbox: LootBoxEngine,
    presence: Presence,
    ranked: Ranked,
    arcade: Arcade,
    celebrations: Celebrations,
    extra_skins: Vec<u64>,
    assets: SceneAssets,
    maps: MapCatalog,
    first_scene: Mutex<Option<PreparedScene>>,
    accounts: Accounts,
    local: Arc<Account>,
    active: Mutex<HashMap<u32, ActiveLobbyPeer>>,
    game: Arc<GameService>,
}

struct ActiveLobbyPeer {
    replaced: Arc<AtomicBool>,
    channel: Fields,
    member: Value,
    outgoing: mpsc::SyncSender<Arc<LobbyMessage>>,
}

struct LoginLease<'a> {
    lobby: &'a Lobby,
    account: Arc<Account>,
    replaced: Arc<AtomicBool>,
    seat: Option<LobbySeat>,
    incoming: mpsc::Receiver<Arc<LobbyMessage>>,
    next_seat_check: Instant,
}

impl<'a> LoginLease<'a> {
    fn new(lobby: &'a Lobby, account: Arc<Account>, identity: Identity, wall_time: u64) -> Self {
        let replaced = Arc::new(AtomicBool::new(false));
        let member = {
            let profile = account.profile();
            lobby_chat::member(player::record(
                &profile,
                identity,
                lobby.collection.portrait_frame(&profile),
                wall_time,
            ))
        };
        let (outgoing, incoming) = mpsc::sync_channel(CHAT_QUEUE_LIMIT);
        let peer = ActiveLobbyPeer {
            replaced: Arc::clone(&replaced),
            channel: general_channel(),
            member,
            outgoing,
        };
        let mut active = lobby.active.lock().expect("lobby accounts");
        if let Some(previous) = active.insert(account.account_lo, peer) {
            previous.replaced.store(true, Ordering::Release);
        }
        account.set_status(1);
        Self {
            lobby,
            account,
            replaced,
            seat: None,
            incoming,
            next_seat_check: Instant::now(),
        }
    }

    fn current(&self) -> bool {
        !self.replaced.load(Ordering::Acquire)
    }

    fn channel(&self) -> Fields {
        self.seat
            .as_ref()
            .map_or_else(general_channel, |seat| match_channel(seat.handoff.match_id))
    }

    fn set_seat(&mut self, seat: Option<LobbySeat>) {
        self.seat = seat;
        let mut active = self.lobby.active.lock().expect("lobby accounts");
        if let Some(peer) = active
            .get_mut(&self.account.account_lo)
            .filter(|peer| Arc::ptr_eq(&peer.replaced, &self.replaced))
        {
            peer.channel = self.channel();
        }
    }

    fn poll_chat(&mut self, transport: &mut LobbyTransport, now: Instant) -> io::Result<()> {
        let schemas = &self.lobby.schemas;
        if let Some(seat) = self.seat.as_ref().filter(|_| now >= self.next_seat_check) {
            self.next_seat_check = now + Duration::from_secs(1);
            if !self.lobby.game.seat_exists(&seat.handoff)? {
                let old_channel = self.channel();
                self.set_seat(None);
                transport.send_message(schemas, &leave_chat_channel(old_channel))?;
                transport.send_message(schemas, &join_chat_channel(general_channel()))?;
            }
        }
        let channel = Value::Struct(self.channel());
        for message in self.incoming.try_iter().take(CHAT_QUEUE_LIMIT) {
            if message.2.get(&0x78) == Some(&channel) {
                transport.send_message(schemas, &message)?;
            }
        }
        Ok(())
    }
}

impl Drop for LoginLease<'_> {
    fn drop(&mut self) {
        if let Some(seat) = self.seat.take() {
            let _ = self.lobby.game.release_seat(&seat.handoff);
        }
        let mut active = self.lobby.active.lock().expect("lobby accounts");
        if active
            .get(&self.account.account_lo)
            .is_some_and(|current| Arc::ptr_eq(&current.replaced, &self.replaced))
        {
            active.remove(&self.account.account_lo);
            self.account.set_status(0);
        }
    }
}

impl Lobby {
    fn chat_request(
        &self,
        lease: &LoginLease<'_>,
        request: &LobbyMessage,
        session: &mut LobbySession<'_>,
    ) -> io::Result<bool> {
        let Some((channel, request)) = ChatRequest::read(request) else {
            return Ok(false);
        };
        let active = self.active.lock().expect("lobby accounts");
        let Some(sender) = active.get(&lease.account.account_lo).filter(|peer| {
            Arc::ptr_eq(&peer.replaced, &lease.replaced) && &peer.channel == channel
        }) else {
            return Ok(true);
        };
        let peers = active.values().filter(|peer| &peer.channel == channel);
        match request {
            ChatRequest::Members => {
                let members: Vec<_> = peers.map(|peer| peer.member.clone()).collect();
                drop(active);
                session.send(&lobby_chat::members(channel.clone(), members))?;
            }
            ChatRequest::Post { text, flags } => {
                queue_chat(
                    peers,
                    lobby_chat::post(channel.clone(), sender.member.clone(), text, flags),
                );
            }
        }
        Ok(true)
    }

    fn serve_peer(&self, stream: PeerStream, id: u32) -> io::Result<()> {
        let Some(mut session) = LobbySession::accept_peer(
            stream,
            id,
            &self.signing_key,
            &self.accounts,
            &self.schemas,
        )?
        else {
            return Ok(());
        };
        let login = match session.prepare_login(&self.accounts, || Ok(Arc::clone(&self.local)))? {
            LoginRequest::Prepared(login) => login,
            LoginRequest::Rejected { reason } => {
                session.send_disconnect(reason)?;
                return Ok(());
            }
            LoginRequest::Disconnected => return Ok(()),
        };
        let wall_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs();
        let (menu, members_may_invite) = {
            let mut profile = login.account.profile();
            profile.last_online = wall_time as i64;
            let menu = MenuHero::default().select(
                &profile,
                self.collection.heroes(),
                self.collection.items(),
            )?;
            let members = profile
                .settings
                .get("+0x10D")
                .and_then(|settings| settings.get("+0x0"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            (menu, members)
        };
        login.account.save()?;
        let mut lease =
            LoginLease::new(self, Arc::clone(&login.account), login.identity, wall_time);
        let messages = login.available_messages(
            &self.schemas,
            &self.templates,
            &self.collection,
            &self.presence,
            &self.ranked,
            LoginContent {
                menu_hero: menu.menu_guid,
                extra_skins: &self.extra_skins,
                wall_time,
                arcade: &self.arcade,
                celebrations: Some(&self.celebrations),
                leaderboard_places: &LeaderboardPlaces::new(),
                solo_party: Some(SoloLoginParty {
                    id: login.identity.party_id,
                    entity: login.identity.party_entity,
                    menu_hero: menu.party_hero,
                    members_may_invite,
                }),
            },
        )?;
        for message in messages {
            if !lease.current() {
                break;
            }
            session.send(&message)?;
        }
        if lease.current() {
            session.send(&join_chat_channel(general_channel()))?;
        }
        let menu_start = Instant::now();
        let mut retries = CHAT_FEATURE_RETRIES.into_iter().peekable();
        while let Some(request) = session.receive_message_until(|transport| {
            if !lease.current() {
                return Ok(false);
            }
            let now = Instant::now();
            // On a cold launch the client can drop the permissions sent during login.
            if retries
                .peek()
                .is_some_and(|delay| now.duration_since(menu_start) >= *delay)
            {
                transport.send_message(
                    &self.schemas,
                    &(player::PERMISSIONS, 55500, player::features()),
                )?;
                retries.next();
            }
            lease.poll_chat(transport, now)?;
            Ok(true)
        })? {
            if self.chat_request(&lease, &request, &mut session)? {
                continue;
            }
            if let Some(settings) = SettingsRequest::read(&request) {
                if let Err(error) = settings.apply(&login.account) {
                    eprintln!("[lobby] ignoring invalid settings update: {error}");
                }
                continue;
            }
            if let Some(equip) = EquipRequest::read(&request) {
                if let Some(equipped) = equip.apply(&login, &self.collection, wall_time)? {
                    if let (Some(seat), Some(skin)) = (&lease.seat, equipped.skin) {
                        self.game.update_skin(&seat.handoff, equip.hero, skin)?;
                    }
                    for message in equipped.messages {
                        session.send(&message)?;
                    }
                }
                continue;
            }
            if let Some(open_box) = OpenBoxRequest::read(&request) {
                for message in self
                    .lootbox
                    .open_box(&login, &self.collection, open_box.box_id)?
                {
                    session.send(&message)?;
                }
                continue;
            }
            if let Some(purchase) = PurchaseRequest::read(&request) {
                for message in self
                    .lootbox
                    .purchase(&login, &self.collection, purchase.guid)?
                {
                    session.send(&message)?;
                }
                continue;
            }
            if let Some(career) = CareerProfileRequest::read(&request) {
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_secs();
                let cards = {
                    let profile = login.account.profile();
                    self.arcade
                        .state(&profile, self.ranked.season(&profile), now_secs)?
                        .competitive_cards
                };
                for message in
                    career.reply(&login, &self.collection, &self.ranked, &cards, now_secs)?
                {
                    session.send(&message)?;
                }
                continue;
            }
            if let Some(reply) = player_names_reply(&request, &self.accounts)? {
                session.send(&reply)?;
                continue;
            }
            if let Some(reply) = owl_live_reply(&request) {
                session.send(&reply)?;
                continue;
            }
            let arcade = lobby_practice::arcade_queue(&request).and_then(|(card, key)| {
                Arcade::junkenstein_queue(card, &login.account.profile(), wall_time)
                    .map(|rules| (rules, key))
            });
            if arcade.is_none() && !lobby_practice::is_request(&request) {
                continue;
            }
            if let Some(old) = &lease.seat {
                if self.game.seat_exists(&old.handoff)? {
                    continue;
                }
            }
            let input = SoloPracticeHandoff::new(match_id()?, "127.0.0.1");
            if let Some((rules, key)) = arcade {
                if let Some(map) = self.maps.game_map(
                    JUNKENSTEIN_MAP,
                    rules.mode,
                    "Junkenstein's Revenge",
                    false,
                    &[rules.players, 0],
                ) {
                    let result = lobby_practice::dispatch_junkenstein(
                        (map, rules, key.clone()),
                        &login,
                        &self.game,
                        input,
                        (&self.assets, &self.collection),
                        |message| session.send(message),
                    );
                    match result {
                        Ok(seat) => lease.set_seat(Some(seat)),
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::Unsupported
                            ) =>
                        {
                            session.send(&lobby_practice::queue_unavailable(key.clone()))?;
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    session.send(&lobby_practice::queue_unavailable(key.clone()))?;
                }
                continue;
            }
            let seat = lobby_practice::dispatch_scene(
                &request,
                &login,
                &self.game,
                input,
                (&self.assets, &self.collection),
                |map| {
                    let first = self.first_scene.lock().expect("prepared scene").take();
                    first.map_or_else(|| self.assets.prepare(map), Ok)
                },
                |message| session.send(message),
            )?;
            lease.set_seat(seat);
        }
        if !lease.current() {
            session.send_disconnect(SIGNED_IN_ELSEWHERE)?;
        }
        Ok(())
    }
}

fn queue_chat<'a>(peers: impl Iterator<Item = &'a ActiveLobbyPeer>, message: LobbyMessage) {
    let message = Arc::new(message);
    for peer in peers {
        let _ = peer.outgoing.try_send(Arc::clone(&message));
    }
}

fn serve(listener: TcpListener, lobby: Arc<Lobby>, shutdown: mpsc::Receiver<()>) -> io::Result<()> {
    let mut peers = PeerWorkers::default();
    let mut id = 0u32;
    while matches!(shutdown.try_recv(), Err(mpsc::TryRecvError::Empty)) {
        peers.reap();
        if let Some((socket, _)) = accept(&listener)? {
            if peers.len() < MAX_LOBBY_PEERS {
                id = id.wrapping_add(1);
                let lobby = Arc::clone(&lobby);
                let _ = peers.spawn("orpheus-lobby-peer", socket, move |stream| {
                    let _ = lobby.serve_peer(stream, id);
                });
            }
        } else if !matches!(
            shutdown.recv_timeout(ACCEPT_POLL),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            break;
        }
    }
    Ok(())
}

fn match_id() -> io::Result<[u64; 2]> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(std::array::from_fn(|i| {
        u64::from_le_bytes(random[i * 8..i * 8 + 8].try_into().unwrap()) >> 1
    }))
}

fn addr(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
