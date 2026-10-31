#!/bin/sh
# Install the already-built app and its launcher for the current user.
set -eu
app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
data_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}
bin_dir=${XDG_BIN_HOME:-"$HOME/.local/bin"}
case "$data_dir:$bin_dir" in
  /*:/*) ;;
  *) printf '%s\n' 'XDG_DATA_HOME and XDG_BIN_HOME must be absolute paths.' >&2; exit 1 ;;
esac
if [ ! -x "$app_dir/tikho" ]; then
  printf '%s\n' 'Build CalmIG first: ./quiet-desktop/build.sh' >&2
  exit 1
fi
# Desktop entry values cannot contain raw newlines.
case "$bin_dir" in
  *'
'*) printf '%s\n' 'The installation path cannot contain a newline.' >&2; exit 1 ;;
esac
mkdir -p "$bin_dir" "$data_dir/applications" "$data_dir/icons/hicolor/64x64/apps"
binary_tmp=$(mktemp "$bin_dir/.calmig.XXXXXX")
entry_tmp=$(mktemp "$data_dir/applications/.calmig.XXXXXX")
trap 'rm -f "$binary_tmp" "$entry_tmp"' EXIT HUP INT TERM
install -m 755 "$app_dir/tikho" "$binary_tmp"
install -m 644 "$app_dir/src-tauri/icons/icon.png" "$data_dir/icons/hicolor/64x64/apps/calmig.png"
# Desktop Exec is a quoted desktop-entry string, not a shell command.
escaped_bin=$(printf '%s' "$bin_dir/calmig" | sed 's/\\/\\\\\\\\/g; s/"/\\\\"/g; s/[$`]/\\\\&/g; s/%/%%/g')
cat > "$entry_tmp" <<DESKTOP
[Desktop Entry]
Type=Application
Name=CalmIG
Comment=A quiet photo calendar and messages
Comment[ru]=Спокойный календарь фотографий и переписки
Exec="$escaped_bin"
Icon=calmig
Terminal=false
Categories=Network;InstantMessaging;
StartupNotify=true
DESKTOP
chmod 644 "$entry_tmp"
mv -f "$binary_tmp" "$bin_dir/calmig"
mv -f "$entry_tmp" "$data_dir/applications/calmig.desktop"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$data_dir/applications" >/dev/null 2>&1 || :
fi
printf 'Installed CalmIG launcher: %s\n' "$data_dir/applications/calmig.desktop"
