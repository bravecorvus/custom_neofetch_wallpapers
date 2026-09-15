# Shuffle Neofetch Pics

A tiny HTTP server that hands out a rotating, shuffled image path, so every
`fastfetch`/`neofetch` invocation gets a different picture.

The original version was a bash loop wrapped around `ls` that wrote the last
image path to a file. That mostly worked, but with a macro setup that opens
several terminal windows at login I had to insert manual pauses to keep the
paths cycling properly. So it became a small concurrent HTTP server instead:
any number of simultaneous `fastfetch` calls can hit it and each one is handed
the next image in the rotation.

It is written in Rust with **zero dependencies** — just the standard library.
The whole thing is a ~450 KB binary that idles at about 2 MB of RSS across two
threads. (The previous Go implementation used ~10 MB across five.)

## How it works

* On startup it reads the image directory, filters to actual image files, and
  shuffles the list.
* The list is a rotating queue: each request pops the front entry and pushes it
  straight to the back. Concurrent callers therefore never collide on the same
  picture, and the full set cycles before anything repeats.
* On every hour boundary it re-reads and re-shuffles the directory, so pictures
  you add show up without a restart.

## Installation

```
cd $HOME
git clone https://github.com/gilgameshskytrooper/custom_neofetch_wallpapers.git
cd custom_neofetch_wallpapers
mkdir -p img
cargo build --release
cp target/release/custom_neofetch_wallpapers .
strip -x custom_neofetch_wallpapers     # optional, macOS: 449 KB -> 320 KB
./custom_neofetch_wallpapers            # or: ./custom_neofetch_wallpapers /path/to/img
```

The release profile sets `strip = "none"` on purpose. Cargo's own `strip = true`
shells out to `rust-objcopy`, which is broken in some rustup installs (it looks
for `libLLVM.dylib` one directory away from where rustup puts it). The system
`strip` above does the same job without the landmine.

Then drop the pictures you want rendered into `$HOME/custom_neofetch_wallpapers/img/`.

Recognised extensions: `jpg jpeg png gif webp bmp tif tiff heic heif avif`.
Hidden files (`.DS_Store` and friends) are skipped.

### Configuration

| | |
|---|---|
| Argument 1 | image directory (default `$HOME/custom_neofetch_wallpapers/img`) |
| `NEOFETCH_WALLPAPER_PORT` | listen port (default `7777`) |

The server binds to `127.0.0.1` only. It is not reachable from the network, and
macOS will not prompt you to allow incoming connections.

## Usage

Any request to any path returns the next image path as `text/plain`:

```
$ curl http://localhost:7777
/Users/you/custom_neofetch_wallpapers/img/makima1.jpg
```

With `fastfetch` (use the logo type your terminal supports — `iterm` for
iTerm2, `kitty` for Kitty, `sixel` elsewhere):

```
fastfetch --config none --logo-type iterm --logo "$(curl -s http://localhost:7777)"
```

With the older `neofetch`:

```
neofetch --config none --shell_version off --iterm2 $(curl -s http://localhost:7777 | xargs) --crop_mode normal
```

Put that line in your `~/.zshrc` to get a fresh picture in every new shell.

## Daemon

### macOS (`launchd`)

A ready-made agent manifest ships with the repo as
[`com.neofetch.plist`](com.neofetch.plist). It contains two placeholders,
`__REPO__` and `__HOME__`, because launchd does **not** expand `~`, `$HOME`, or
any environment variable inside these keys — every path has to be absolute and
literal.

Install it from the repo root:

```
sed -e "s|__REPO__|$PWD|g" -e "s|__HOME__|$HOME|g" com.neofetch.plist \
  > ~/Library/LaunchAgents/com.neofetch.plist
xattr -c ~/Library/LaunchAgents/com.neofetch.plist
mkdir -p ~/Library/Logs
launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.neofetch.plist
```

Confirm it came up:

```
launchctl print gui/$UID/com.neofetch | grep -E "state|pid"   # state = running
curl http://localhost:7777                                     # prints an image path
```

That is it — it now starts at every login and restarts itself if it dies.

#### Reloading after a change

Editing the installed plist, or dropping in a new binary, needs an explicit
restart. launchd will not notice on its own.

```
# after replacing the binary only
launchctl kickstart -k gui/$UID/com.neofetch

# after editing the plist
launchctl bootout gui/$UID/com.neofetch
launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.neofetch.plist
```

To uninstall:

```
launchctl bootout gui/$UID/com.neofetch
rm ~/Library/LaunchAgents/com.neofetch.plist
```

#### If `bootstrap` fails

`launchctl` reports nearly every problem as the same unhelpful line:

```
Bootstrap failed: 5: Input/output error
```

On current macOS it is almost always one of these three:

1. **The filename does not match the `Label`.** `com.neofetch.plist` must
   contain `<string>com.neofetch</string>`. Older macOS releases tolerated a
   mismatch; current ones reject the job outright. This is why the file is not
   named `org.neofetch.plist` any more.
2. **The plist is quarantined.** A file that arrived via a browser, AirDrop, or
   any download carries a `com.apple.quarantine` extended attribute, and
   launchd refuses to read it. Hence the `xattr -c` above.
3. **The job is already loaded.** Run `launchctl bootout gui/$UID/com.neofetch`
   first, then bootstrap again.

Check the first two with:

```
plutil -lint ~/Library/LaunchAgents/com.neofetch.plist   # should say OK
xattr -l ~/Library/LaunchAgents/com.neofetch.plist       # should print nothing
```

If the job bootstraps but the port stays dead, the server itself is failing to
start — its output is in `~/Library/Logs/com.neofetch.log`, and
`launchctl print gui/$UID/com.neofetch` shows the last exit code.

### Linux (`systemd`)

`~/.config/systemd/user/neofetch-wallpapers.service`:

```ini
[Unit]
Description=Shuffle Neofetch Pics

[Service]
ExecStart=%h/custom_neofetch_wallpapers/custom_neofetch_wallpapers %h/custom_neofetch_wallpapers/img
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
```

```
systemctl --user enable --now neofetch-wallpapers.service
```
