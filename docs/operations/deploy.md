# Deploy

The hosted demo runs on **NEAR testnet**. A person performs every step that creates an account,
spends money, sets DNS or publishes a URL; the repository only provides the configuration. Pick
one option: Fly.io (A) or a plain VPS with Caddy (B).

Build status at this commit: **not built on this host**. The Docker CLI (29.7.2) is installed,
but its daemon was not running, so the image has not been built here. Both compose files pass
`docker compose config`, and `deploy/smoke.py` passes against a locally run `aps-api`.

> **In-container only until APS_BIND lands.** Until the wave-2 change that reads `APS_BIND`, the
> API binds `127.0.0.1` inside the container. The published port, the Fly.io HTTP service and
> Caddy therefore cannot reach it yet. Only the in-container smoke test below works. The same
> wiring adds `APS_DB_PATH`; until then, state is in memory and lost on restart.

## Files

| File | Purpose |
|---|---|
| `Dockerfile` | build stage (Rust 1.98.0), then a slim runtime (Python 3.12.15), both Debian trixie and pinned by digest |
| `.dockerignore` | keeps docs, tests, `.env*` and databases out of the build context |
| `docker-compose.yml` | local run: read-only root filesystem, tmpfs `/tmp`, `/data` volume, no capabilities |
| `deploy/fly/fly.toml` | Fly.io app, one machine, `/data` volume |
| `deploy/vps/docker-compose.yml`, `deploy/vps/Caddyfile` | VPS: the API on an internal network behind Caddy with automatic HTTPS |
| `deploy/healthcheck.py` | image healthcheck on `/health` |
| `deploy/smoke.py` | the demo flow as a stdlib smoke test; `--read-only` for public URLs |

## Local container

From the repository root:

```
docker compose up --build -d
docker compose ps                                   # wait for "healthy"
docker compose exec -T aps python3 - < deploy/smoke.py
docker compose exec aps aps-admin verify-journal --db /data/aps.sqlite
docker compose down                                 # add -v to delete the data volume
```

The smoke test prints one `PASS` or `FAIL` line per check and exits 0 when all pass. Once
`APS_BIND` lands, `python deploy/smoke.py --base http://127.0.0.1:8080` also works from the host.

## Option A: Fly.io

Prerequisites: a Fly.io account with billing set up, and `flyctl` installed (see
https://fly.io/docs/flyctl/install/).

1. `fly auth login`
2. Choose an app name and put it in `deploy/fly/fly.toml` (`app = "..."`). Keep that edit local,
   or commit it if the name is public anyway.
3. `fly apps create <app-name>`
4. `fly volumes create aps_data --app <app-name> --region ams --size 1`
   (use the region set in `primary_region`). Create exactly one volume, because SQLite has a
   single writer.
5. Optional: set NEAR provider settings as secrets, testnet only:
   `fly secrets set --app <app-name> NEAR_NETWORK=testnet NEAR_GATEWAY_ACCOUNT=<gateway>.testnet NEAR_ASSET_CONTRACT=<token>.testnet NEAR_RPC_URL=https://test.rpc.fastnear.com`.
   Leave them unset to run the mock provider.
6. From the repository root: `fly deploy --config deploy/fly/fly.toml --dockerfile Dockerfile`
7. `fly scale count 1 --app <app-name>`. Never run more than one machine.
8. Create the first caller:
   `fly ssh console --app <app-name> -C "aps-admin create-caller --db /data/aps.sqlite --caller judge-demo"`.
   Store the printed key privately.
9. Verify read-only from a workstation:
   `python deploy/smoke.py --base https://<app-name>.fly.dev --read-only`
10. Record `head_seq` and `head_hash` from `aps-admin verify-journal` off-host (runbook).

Custom domain (optional): `fly certs add <domain> --app <app-name>`, then create the DNS records
it prints at the domain registrar.

## Option B: a plain VPS with Caddy

Prerequisites:

- a small Linux VPS with a public IPv4 address (1 vCPU and 1 GB RAM is enough);
- Docker Engine with the compose plugin;
- a domain name you control.

1. Create a DNS `A` record (and `AAAA` if the VPS has IPv6) for the demo host name, pointing at
   the VPS. Wait until `dig +short <host>` returns the VPS address.
2. Open only ports 22, 80 and 443 in the VPS firewall.
3. On the VPS: `git clone <repository URL>` and `cd` into the repository.
4. Start both services:
   `APS_DOMAIN=<host> docker compose -f deploy/vps/docker-compose.yml up -d --build`.
   Caddy obtains the TLS certificate automatically on the first request.
5. `docker compose -f deploy/vps/docker-compose.yml ps`: `aps` must report healthy.
6. Create the first caller:
   `docker compose -f deploy/vps/docker-compose.yml exec aps aps-admin create-caller --db /data/aps.sqlite --caller judge-demo`
7. Verify read-only from a workstation:
   `python deploy/smoke.py --base https://<host> --read-only`
8. Schedule a daily backup with `aps-admin backup` (runbook), and copy it off the VPS.

To update: `git pull`, then rerun step 4. The data lives in the `aps-data` volume and survives
rebuilds.

## After any deploy

- The public URL, the image digest (`docker image inspect triagepay:vps --format
  '{{.Id}}'`, or the digest `fly deploy` prints), and the deploy steps actually performed go into
  the status tracker.
- Only read-only checks run against the public URL from automation. `deploy/smoke.py
  --read-only` issues GET requests only.
