# sentrypanel

![Tests](./.github/badges/tests.svg)
![Rust](./.github/badges/rust.svg)
![Platforms](./.github/badges/platforms.svg)
![Dependencies](./.github/badges/dependencies.svg)

Key Sentry numbers on one screen of a Divoom Times Gate, refreshed every minute.

![The panel](screenshot.png)

## Install

Requires Rust 1.88 or newer, macOS or Linux.

```bash
cargo install --git https://github.com/ivan-moskvin/sentrypanel --locked
```

## Token

Create a personal token in Sentry: **User Settings → Personal Tokens**, scope
`org:read`. Organization tokens cannot read events.

Pass it in `SENTRY_AUTH_TOKEN`, or put it into a file only you can read and
name the file in `sentry.token_file`:

```bash
mkdir -p ~/.config/sentrypanel
printf '%s\n' 'sntryu_...' > ~/.config/sentrypanel/token
chmod 600 ~/.config/sentrypanel/token
```

A token file readable by others is refused.

## Config

Copy [config.example.toml](config.example.toml) to:

- macOS: `~/Library/Application Support/sentrypanel/config.toml`
- Linux: `~/.config/sentrypanel/config.toml`

or pass another path with `--config PATH` to any command.

```toml
refresh_secs = 60                      # at least 10

[sentry]
base_url = "https://de.sentry.io"      # the region of the organization
org = "my-org"
token_file = "~/.config/sentrypanel/token"

[device]
# ip = "192.168.1.50"                  # or name = "Times Gate"; neither with one device
screen = 1                             # 1–5
# port = 8478                          # where the device fetches frames from

[[section]]
title = "SERVERS"                      # up to 12 characters

[[section.metric]]
label = "WEB"                          # up to 6 characters
dataset = "logs"                       # errors, logs, spans, ...
query = "host:web-1"                   # Sentry search syntax
field = "avg(tags[ping_ms,number])"    # one aggregate
period = "5m"                          # s, m, h, d, w; default 1h
unit = "ms"                            # ms, s, %, or up to 3 characters; empty for a count
warn = 80
crit = 200
```

More keys of a metric:

| Key           | Meaning                                                              |
|---------------|----------------------------------------------------------------------|
| `project`     | project slug or numeric id; all projects when left out               |
| `worse`       | `higher` (default) or `lower`, for thresholds like free disk space   |
| `missing`     | `grey` (default), `warn` or `crit` — what no data means              |
| `scale`       | multiplier before display, e.g. `100` for `failure_rate()`           |
| `active_when` | a query; a dot is drawn before the label while it matches anything   |

The screen holds 12 metrics in 3 sections. Values are green, amber at `warn`,
red at `crit`, grey without data. When Sentry cannot be reached or refuses the
token, the whole panel turns grey and the corner says `OFFLINE`, `AUTH`, `LIMIT`
or `HTTP <code>` instead of the time of the last refresh.

## Usage

```
sentrypanel run                     keep the screen updated
sentrypanel once                    fetch once and print the numbers
sentrypanel preview [--demo] FILE   draw the panel into a GIF file
sentrypanel devices                 list the Divoom devices on the network
sentrypanel screen [N]              show or change the screen, 1–5
sentrypanel service install         run as a service of this user
sentrypanel service uninstall       remove the service
sentrypanel version                 print the version
```

`once` is the way to check a config. `preview --demo` draws made-up numbers and
needs no token.

`service install` registers `run` as a launchd agent on macOS (log in
`~/Library/Logs/sentrypanel.log`) or a systemd user unit on Linux. It starts at
login and is restarted if it exits. When the panel stops, the screen gets its
clock face back.

## License

MIT
