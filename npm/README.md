# mag-memory

**MAG -- Memory Augmented Generation.** Local MCP memory server with ONNX embeddings.

This npm package installs the prebuilt `mag` binary for your platform.

## Install

```bash
npm install mag-memory
```

The postinstall script automatically downloads the correct binary for your
OS and architecture from [GitHub Releases](https://github.com/George-RD/mag/releases).

### Supported platforms

| OS      | x64 | arm64 |
|---------|-----|-------|
| macOS   | yes | yes   |
| Linux   | yes | yes   |
| Windows | yes | --    |

## Usage

After installation, `mag` is available on your PATH:

```bash
# Start the MCP server (stdio transport)
mag serve

# Show help
mag --help
```

### Use with Claude Desktop

Add to your Claude Desktop MCP config (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "mag": {
      "command": "npx",
      "args": ["-y", "mag-memory", "serve"]
    }
  }
}
```

## Local development

Set the `MAG_BINARY_PATH` environment variable to skip the download and
symlink a local build instead:

```bash
MAG_BINARY_PATH=./target/release/mag npm install
```

## License

MIT
