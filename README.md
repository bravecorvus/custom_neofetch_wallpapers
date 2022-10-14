# Shuffle Neofetch Pics

This is a program that allows you shuffle an entire directory worth of pictures when using [neofetch](https://github.com/dylanaraps/neofetch).

The initial version of this program just used basic bash logic that wrapped a blocking `for` loop around an `ls` command on a directory of pictures.
The bash script wrote the image path last passed to `neofetch` to a file. This worked somewhat, but in using my quite complex macros setup
which triggers multiple terminal windows to open up at login, I basically had to write in manual pauses in the macro to ensure the image
paths were getting cycled properly.

Hence, I wrote a more full-fledged concurrent implementation of the image shuffling using Go channels on a light HTTP server
such that it can be called by as many simultaneous instantiations of `neofetch` for my purposes.

## Installation
If this becomes popular for some reason, I will start drafting releases with packaged binaries. But for now:

```
cd $HOME
git clone https://github.com/gilgameshskytrooper/custom_neofetch_wallpapers.git
cd custom_neofetch_wallpapers
mkdir img
go build
./custom_neofetch_wallpapers (PASS DIRECTORY YOU CLONED REPO TO IF IT WAS NOT $HOME AS ARGUMENT 1)
```

After these steps, add photos you want to render in `neofetch` in `$HOME/custom_neofetch_wallpapers/img/`

## Usage

Since this is a server, you will need to execute this as a daemon in the background.

This will run as a server on port `:7777`. To use with `neofetch`, you can do the following

```
neofetch --config none --shell_version off --iterm2 $(curl http://localhost:7777 | xargs) --crop_mode normal
```

Notice I am using a direct HTTP 1.1 GET request to `http://localhost:7777` to figure out which file path should be passed to `neofetch`.


## Daemon

For Linux, you should should use `systemd` to run this.

For macOS, you can add the folliwng `launchd` script in `$HOME/Library/LaunchAgents/org.neofetch.plist`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.neofetch</string>
    <key>ProgramArguments</key>
    <array>
        <string>/PATH_TO/custom_neofetch_wallpapers/custom_neofetch_wallpapers</string>
    </array>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

**Make sure to replace **PATH_TO** with the filepath to the `custom_neofetch_wallpapers` directory**
