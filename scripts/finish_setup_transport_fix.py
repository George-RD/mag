from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


path = Path("src/setup.rs")
text = path.read_text()
text = replace_once(
    text,
    "    // Model download phase — always runs; daemon must start after models are ready.\n",
    "    // Model download phase — always runs before setup completes.\n",
    "model phase comment",
)
text = replace_once(
    text,
    '''                transport: TransportMode::Command,
                port: 4242,
                no_start: true,
                uninstall: false,
''',
    '''                transport: TransportMode::Command,
                uninstall: false,
''',
    "full setup test fields",
)
if "no_start" in text:
    raise SystemExit("unexpected no_start reference remains in src/setup.rs")
path.write_text(text)

Path("scripts/finish_setup_transport_fix.py").unlink()
Path(".github/workflows/finish-setup-transport-fix.yml").unlink()
