from pathlib import Path
import re


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


setup_path = Path("src/setup.rs")
setup = setup_path.read_text()
setup = replace_once(
    setup,
    "can communicate with the MAG daemon.",
    "can communicate with the MAG stdio server.",
    "setup module description",
)
setup = replace_once(
    setup,
    '''pub struct SetupArgs {
    pub non_interactive: bool,
    pub tools: Option<Vec<String>>,
    pub transport: TransportMode,
    pub port: u16,
    pub no_start: bool,
    pub uninstall: bool,
    pub force: bool,
    /// Only patch `~/.claude/settings.json` sandbox allowlist; skip full setup.
    pub fix_sandbox: bool,
}
''',
    '''pub struct SetupArgs {
    pub non_interactive: bool,
    pub tools: Option<Vec<String>>,
    pub transport: TransportMode,
    pub uninstall: bool,
    pub force: bool,
    /// Only patch `~/.claude/settings.json` sandbox allowlist; skip full setup.
    pub fix_sandbox: bool,
}
''',
    "SetupArgs fields",
)
setup = replace_once(
    setup,
    '''    if args.fix_sandbox {
        return run_fix_sandbox();
    }

    // Detect phase
''',
    '''    if args.fix_sandbox {
        return run_fix_sandbox();
    }

    // Fail before detection, connector installation, model work, or config writes.
    ensure_setup_transport_available(args.transport)?;

    // Detect phase
''',
    "run_setup transport guard",
)
setup = replace_once(
    setup,
    '''    // Daemon phase — starts after models are downloaded.
    #[cfg(feature = "daemon-http")]
    maybe_start_daemon(args.port, args.no_start)?;

    Ok(())
''',
    '''    Ok(())
''',
    "remove incomplete daemon phase",
)
setup = replace_once(
    setup,
    '''fn configure_tools(
    tools: &[&DetectedTool],
    mode: TransportMode,
    all_detected: &[&DetectedTool],
) -> Result<ConfigureSummary> {
    let mut summary = ConfigureSummary::default();
''',
    '''fn configure_tools(
    tools: &[&DetectedTool],
    mode: TransportMode,
    all_detected: &[&DetectedTool],
) -> Result<ConfigureSummary> {
    ensure_setup_transport_available(mode)?;
    let mut summary = ConfigureSummary::default();
''',
    "configure_tools transport guard",
)
setup = replace_once(
    setup,
    '''// ---------------------------------------------------------------------------
// Daemon management
// ---------------------------------------------------------------------------

#[cfg(feature = "daemon-http")]
fn maybe_start_daemon(port: u16, no_start: bool) -> Result<()> {
    if no_start {
        tracing::debug!("--no-start: skipping daemon check");
        return Ok(());
    }

    // Check if daemon is already running via daemon.json
    match crate::daemon::DaemonInfo::read() {
        Ok(Some(info)) if !info.is_stale() => {
            println!(
                "  MAG daemon already running (pid {}, port {}).\\n",
                info.pid, info.port
            );
            return Ok(());
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to read daemon info; assuming not running");
        }
        _ => {}
    }

    println!("  Tip: start the MAG daemon with `mag serve` (port {port}).\\n");

    Ok(())
}

''',
    '''''',
    "remove daemon helper",
)
setup = replace_once(
    setup,
    '''/// Parses a CLI transport string into a `TransportMode`.
pub fn parse_transport(s: &str, port: u16) -> Result<TransportMode> {
    match s.to_lowercase().as_str() {
        "command" | "cmd" => Ok(TransportMode::Command),
        "http" => Ok(TransportMode::Http { port }),
        "stdio" => Ok(TransportMode::Stdio),
        other => {
            anyhow::bail!("unknown transport mode: '{other}' (expected command, http, or stdio)")
        }
    }
}
''',
    '''fn unavailable_setup_transport(mode: &str) -> anyhow::Error {
    match mode {
        "http" => anyhow::anyhow!(
            "HTTP setup transport is not available because MAG does not currently assemble an HTTP MCP server. Use `--transport command`; HTTP remains a separate optional service milestone."
        ),
        "stdio" => anyhow::anyhow!(
            "stdio setup transport is not available because `mag serve` already provides the stdio MCP command and does not accept `--stdio`. Use `--transport command`."
        ),
        other => anyhow::anyhow!("setup transport '{other}' is not available"),
    }
}

fn ensure_setup_transport_available(mode: TransportMode) -> Result<()> {
    match mode {
        TransportMode::Command => Ok(()),
        TransportMode::Http { .. } => Err(unavailable_setup_transport("http")),
        TransportMode::Stdio => Err(unavailable_setup_transport("stdio")),
    }
}

/// Parses a CLI transport string into the only currently executable setup mode.
pub fn parse_transport(s: &str) -> Result<TransportMode> {
    let normalized = s.to_lowercase();
    match normalized.as_str() {
        "command" | "cmd" => Ok(TransportMode::Command),
        "http" | "stdio" => Err(unavailable_setup_transport(normalized.as_str())),
        other => anyhow::bail!("unknown transport mode: '{other}' (expected command)"),
    }
}
''',
    "transport parser",
)

parse_replacements = {
    'parse_transport("command", 4242)': 'parse_transport("command")',
    'parse_transport("cmd", 4242)': 'parse_transport("cmd")',
    'parse_transport("http", 9090)': 'parse_transport("http")',
    'parse_transport("stdio", 4242)': 'parse_transport("stdio")',
    'parse_transport("COMMAND", 8080)': 'parse_transport("COMMAND")',
    'parse_transport("grpc", 4242)': 'parse_transport("grpc")',
}
for old, new in parse_replacements.items():
    setup = replace_once(setup, old, new, f"test call {old}")

setup, initializer_count = re.subn(
    r'(?m)^            port: \d+,\n            no_start: (?:true|false),\n',
    '',
    setup,
)
if initializer_count < 4:
    raise SystemExit(f"SetupArgs test initializers: expected at least 4 matches, found {initializer_count}")
setup = replace_once(
    setup,
    "        assert_eq!(args.port, 4242);\n",
    "        assert_eq!(args.transport, TransportMode::Command);\n",
    "SetupArgs default assertion",
)
setup_path.write_text(setup)

cli_path = Path("src/cli.rs")
cli = cli_path.read_text()
cli = replace_once(
    cli,
    '''        /// Transport mode: command (default), http, or stdio.
        #[arg(long, default_value = "command")]
        transport: String,
        /// Port for HTTP transport mode.
        #[arg(long, default_value_t = 4242)]
        port: u16,
        /// Do not attempt to start or check the MAG daemon.
        #[arg(long)]
        no_start: bool,
''',
    '''        /// Transport mode. Only command is currently available.
        #[arg(long, default_value = "command")]
        transport: String,
''',
    "setup CLI transport flags",
)
cli = replace_once(
    cli,
    '''    #[test]
    fn test_cli_uninstall_command() {
''',
    '''    #[test]
    fn test_cli_setup_help_exposes_only_command_transport() {
        let error = match Cli::try_parse_from(["mag", "setup", "--help"]) {
            Ok(_) => panic!("setup --help should exit through clap"),
            Err(error) => error,
        };
        let help = error.to_string();
        assert!(
            help.contains("Only command is currently available"),
            "unexpected setup help: {help}"
        );
        assert!(!help.contains("--port"), "unexpected setup help: {help}");
        assert!(
            !help.contains("--no-start"),
            "unexpected setup help: {help}"
        );
        assert!(
            !help.contains("http, or stdio"),
            "unexpected setup help: {help}"
        );
    }

    #[test]
    fn test_cli_uninstall_command() {
''',
    "setup help regression test",
)
cli_path.write_text(cli)

main_path = Path("src/main.rs")
main = main_path.read_text()
main = replace_once(
    main,
    '''    if let Commands::Setup {
        non_interactive,
        tools,
        transport,
        port,
        no_start,
        uninstall,
        force,
        fix_sandbox,
    } = cli.command
    {
        let transport_mode = mag::setup::parse_transport(&transport, port)?;
        return mag::setup::run_setup(mag::setup::SetupArgs {
            non_interactive,
            tools,
            transport: transport_mode,
            port,
            no_start,
            uninstall,
            force,
            fix_sandbox,
        })
''',
    '''    if let Commands::Setup {
        non_interactive,
        tools,
        transport,
        uninstall,
        force,
        fix_sandbox,
    } = cli.command
    {
        let transport_mode = mag::setup::parse_transport(&transport)?;
        return mag::setup::run_setup(mag::setup::SetupArgs {
            non_interactive,
            tools,
            transport: transport_mode,
            uninstall,
            force,
            fix_sandbox,
        })
''',
    "main setup dispatch",
)
main_path.write_text(main)

docs_path = Path("docs/SETUP.md")
docs = docs_path.read_text()
setup_sentence = "This detects installed AI tools, shows their status, and writes the correct MCP config for each one. If you used the shell installer, this already ran."
count = docs.count(setup_sentence)
if count != 2:
    raise SystemExit(f"setup guide paragraph: expected two matches, found {count}")
docs = docs.replace(
    setup_sentence,
    setup_sentence
    + "\n\nSetup currently writes command transport (`mag serve`) only. HTTP service mode remains a separate optional milestone.",
)
docs_path.write_text(docs)

Path("scripts/apply_setup_transport_fix.py").unlink()
Path(".github/workflows/apply-setup-transport-fix.yml").unlink()
