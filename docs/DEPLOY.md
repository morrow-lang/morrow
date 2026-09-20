# Deploy the demo and publish the docs

Two GitHub Actions workflows publish Morrow's public presence. Both stay
disabled until the repository owner sets one variable and one secret, so a
fork or a local checkout never deploys anything by accident.

## The checklist demo on Fly.io

[`deploy-web.yml`](../.github/workflows/deploy-web.yml) builds the static
x86-64 server exactly as CI does, then runs `flyctl deploy` with the
[`fly.toml`](../fly.toml) at the repository root. The image is `FROM scratch`
plus the one executable ([`deploy/fly/Dockerfile`](../deploy/fly/Dockerfile));
browser assets, the compiled Morrow application and the service worker are
embedded in it.

One-time setup, from a machine with `flyctl` signed in:

```sh
fly apps create morrow-demo
fly volumes create morrow_data --app morrow-demo --region ams --size 1
fly tokens create deploy --app morrow-demo   # store as the FLY_API_TOKEN secret
```

Then set the GitHub repository variable `FLY_DEPLOY` to `true` and the secret
`FLY_API_TOKEN`. Every push to `main` that touches the server, the compiler or
the web application redeploys; `workflow_dispatch` deploys on demand.

To use another app name or region, change `app`, `primary_region` and
`MORROW_WEB_ORIGIN` in `fly.toml` together. The origin must be the exact public
`https://` origin browsers will see, because the server enforces it on every
session and WebSocket upgrade.

What the configuration guarantees, and what it does not:

- **Exactly one machine.** Rooms have fixed owners inside one process, and the
  live viewer count is that process's connection count. The workflow deploys
  with `--ha=false`; do not scale above one machine. Multi-node deployment is
  the separate [cluster](CLUSTER.md) path with its own bundle and TLS setup.
- **Durable rooms.** `/data` is a Fly volume; acknowledged room changes are
  checkpointed there and recovered after a restart. Sessions and command
  namespaces still restart with the process, as documented in the
  [web guide](WEB_PREVIEW.md).
- **Always-on machine.** Fly Machines are not Kubernetes pods. `auto_stop_machines = "off"` and `min_machines_running = 1` keep the one process running even with no visitors, so live presence and WebSocket rooms survive idle periods. The volume already recovers room contents after a restart; sessions and in-memory occupancy do not.
- **Open rooms.** The demo has no accounts and no shared access key. Anyone
  who can load the origin can edit the shared room within the server's bounds:
  100 tasks per room, 256-byte labels, admission and connection limits. Treat
  the public URL as a shared whiteboard.
- **TLS at the edge.** Fly terminates HTTPS and forwards plain HTTP to port
  8080. The server marks its session cookie `Secure` because the configured
  origin is `https://`.
- **Runs as root inside a scratch image.** The image has no users database and
  the volume is root-owned. There is no shell, package manager or other
  executable in the image.

The [system dashboard](ADMIN_DASHBOARD.md) at `/admin` shows uptime, memory,
workers, rooms and connections for the running demo after the application has
opened a session. It is read-only: no room contents, credentials or filesystem
paths. `MORROW_WEB_ADMIN=0` hides it.

## The documentation site

[`docs.yml`](../.github/workflows/docs.yml) runs `cargo xtask docs`, which
renders the README, guides, design, roadmap, decision records and the Rust API
reference into one site, and pushes the result to the
[`morrow-lang/morrow-lang.github.io`](https://github.com/morrow-lang/morrow-lang.github.io)
repository's `main` branch.

Setup: create a fine-grained personal access token with **Contents: write**
on **morrow-lang/morrow-lang.github.io** (not only the source repository),
store it as the `DOCS_DEPLOY_TOKEN` secret, and set the
repository variable `DOCS_PUBLISH` to `true`. A token that can read Morrow
but cannot write the site repository will fail the publish step with 403.
The site then updates on every push to `main`. The workflow does not write a
`CNAME` file; add one once the `morrow-lang.org` domain is registered and its
DNS points at GitHub Pages.

## Building the same artifacts locally

```sh
# Static x86-64 server, on Linux with musl-tools installed
rustup target add wasm32-unknown-unknown x86_64-unknown-linux-musl
CC_x86_64_unknown_linux_musl=musl-gcc \
MORROW_WEB_TARGET=x86_64-unknown-linux-musl cargo xtask web-build
fly deploy --local-only --ha=false

# Documentation site
cargo xtask docs dist/docs
```

`cargo xtask web-check dist/morrow-web` exercises two real browser windows,
the live viewer count, offline reload and cache integrity against a locally
built server; set `MORROW_BROWSER` to a Chromium-based browser executable.
