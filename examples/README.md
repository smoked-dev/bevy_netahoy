# Examples

Run the NetAhoy server and client separately:

```bash
cargo run --example ahoy_server
cargo run --example ahoy_client
```

From WSL2, build and run one native Windows server plus two native Windows client windows:

```bash
just win-dev
```

From WSL2, build and serve the WASM browser client while running a local websocket server:

```bash
just win-web
```

Open more browser tabs to join as more players. Player IDs are assigned by the server on join.

This example is intentionally code-only: primitive geometry, explicit Ahoy usercmd packets with backups, server-authoritative snapshots, local predicted Ahoy replay, remote interpolation buffers, and an Aeronet websocket transport backend.
