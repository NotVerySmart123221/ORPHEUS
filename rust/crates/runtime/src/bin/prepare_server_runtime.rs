use orpheus_runtime as runtime;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: prepare_server_runtime --source DIR --destination DIR [--map INDEX]...\n\
    [--game-graphs FILE] [--target-tags-xml DIR] [--mode-rulesets FILE] [--state-groups-xml DIR]\n\
    \nCopy runtime inputs from a prepared local checkout or runtime pack into a new directory.\n\
    The optional XML directory supplies target tags, component 92 metadata and catalog-selected NPC movers from local DataTool .003 exports.\n\
    A local mode rules file supplies a JSON array of profiles for distinct modes.\n\
    A local .01B graph XML directory restores state-group (subgraph box) begin edges into the graph bundle.";

struct Arguments {
    source: PathBuf,
    destination: PathBuf,
    maps: Vec<String>,
    game_graphs: Option<PathBuf>,
    target_tags_xml: Option<PathBuf>,
    mode_rulesets: Option<PathBuf>,
    state_groups_xml: Option<PathBuf>,
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Option<Arguments>, String> {
    let (
        mut source,
        mut destination,
        mut game_graphs,
        mut target_tags_xml,
        mut mode_rulesets,
        mut state_groups_xml,
    ) = (None, None, None, None, None, None);
    let mut maps = Vec::new();
    while let Some(argument) = args.next() {
        let argument = argument.to_str().ok_or("Expected a command-line option")?;
        let (option, inline) = match argument.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (argument, None),
        };
        if matches!(option, "--help" | "-h") {
            return Ok(None);
        }
        let mut value = || {
            inline
                .map(OsString::from)
                .or_else(|| args.next())
                .ok_or_else(|| format!("{option} needs a value"))
        };
        match option {
            "--source" => source = Some(PathBuf::from(value()?)),
            "--destination" => destination = Some(PathBuf::from(value()?)),
            "--game-graphs" => game_graphs = Some(PathBuf::from(value()?)),
            "--target-tags-xml" => target_tags_xml = Some(PathBuf::from(value()?)),
            "--mode-rulesets" => mode_rulesets = Some(PathBuf::from(value()?)),
            "--state-groups-xml" => state_groups_xml = Some(PathBuf::from(value()?)),
            "--map" => maps.push(value()?.into_string().map_err(|_| "Invalid map index")?),
            _ => return Err(format!("Unknown option {option}")),
        }
    }
    Ok(Some(Arguments {
        source: source.ok_or("--source is required")?,
        destination: destination.ok_or("--destination is required")?,
        maps,
        game_graphs,
        target_tags_xml,
        mode_rulesets,
        state_groups_xml,
    }))
}

fn run() -> Result<(), String> {
    let Some(args) = parse(std::env::args_os().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    let destination = runtime::prepare_with_graph_metadata(
        &args.source,
        &args.destination,
        &args.maps,
        args.game_graphs.as_deref(),
        runtime::GraphMetadata {
            target_tags_xml: args.target_tags_xml.as_deref(),
            mode_rulesets: args.mode_rulesets.as_deref(),
            state_groups_xml: args.state_groups_xml.as_deref(),
        },
    )
    .map_err(|error| error.to_string())?;
    println!("Prepared runtime data at {}", destination.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
