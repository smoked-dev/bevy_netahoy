# bevy_netahoy

`bevy_netahoy` is an early Bevy KCC/netcode experiment for cheap,
high-performance Source-style prediction and rollback for high-speed movement
shooters.

The goal is to make rollback and prediction practical for games that need to run in
environments like web browsers. And to support strafe jumping, bhopping,
surfing, sub-ticked hitscan weapons, rockets, rocket jumps, and all 
kinds of other fun stuff. Only made possible by [bevy_ahoy](https://github.com/janhohenheim/bevy_ahoy) and the
`move_and_slide` implementation in [Avian 3D](https://github.com/avianphysics/avian/pull/894).

## Current Work

- Server-authoritative KCC movement with client prediction and replay.
- Remote interpolation for other players.
- A sub-ticked lag-compensated hitscan weapon example.
- Native and browser/WebSocket example paths.

## Still To Do

- Lag-compensated rockets. This is a bigger problem than hitscan because it
  involves impulse changes and predicting those changes through rollback.
  Delagging rockets and rocket jumps has it's own Quake modding trivia behind
  it, including spawning rockets slightly ahead relative to latency. 
- Player-player collision. Players can collide now, but prediction does not
  handle it properly yet, so server correction can turn into visible shaking.
  The open question is how remote player bodies should participate in rollback.
- Melee examples, such as AABB tests for punches or sword hits.
- Decide whether actions like fire and interact should live inside the user
  input stream, or stay separate from the KCC command path. Keeping them
  separate is simpler, but loses the reliability of Quake/Source-style packets
  that back up several previous user commands.

## Run

```bash
cargo run --example ahoy_server
cargo run --example ahoy_client
```

See [examples/README.md](examples/README.md) for the current example commands.
