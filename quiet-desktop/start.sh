#!/bin/sh
set -eu
app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -x "$app_dir/tikho" ]; then
  printf '%s\n' 'Сборка не найдена. Сначала выполните quiet-desktop/build.sh.' >&2
  exit 1
fi
exec "$app_dir/tikho"
