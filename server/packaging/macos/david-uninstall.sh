#!/bin/sh
# Removes David from this Mac: stops the daemon and takes the program, the launchd files and the log settings away.
# The data folder (the database, the certificate) and /etc/david.toml stay, unless --purge is given: then they go too,
# and so do the account `_david` and the logs. The music files of the library are never touched.
set -u
if [ "$(id -u)" != 0 ]; then
    exec sudo "$0" "$@"
fi
PURGE=0
[ "${1:-}" = "--purge" ] && PURGE=1
LABEL=com.the-abraham-box.david
DATA="/Library/Application Support/David"
if [ -f /etc/david.toml ]; then
    d=$(sed -n 's/^data_dir[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' /etc/david.toml | head -n 1)
    [ -n "$d" ] && DATA="$d"
fi

launchctl bootout "system/$LABEL" >/dev/null 2>&1 || true
launchctl bootout "system/$LABEL-reindex" >/dev/null 2>&1 || true
rm -f "/Library/LaunchDaemons/$LABEL.plist" "/Library/LaunchDaemons/$LABEL-reindex.plist"
rm -f /etc/newsyslog.d/david.conf
rm -rf /usr/local/share/david
rm -f /usr/local/bin/david
pkgutil --forget com.the-abraham-box.david >/dev/null 2>&1 || true

if [ "$PURGE" = 1 ]; then
    rm -rf "$DATA"
    rm -f /etc/david.toml /var/log/david.log* /var/log/david-reindex.log*
    dscl . -delete /Users/_david >/dev/null 2>&1 || true
    dscl . -delete /Groups/_david >/dev/null 2>&1 || true
    echo "David is removed, with its data folder, its config, its logs and the account _david."
else
    echo "David is removed. The data folder ($DATA) and /etc/david.toml are kept; run this again with --purge to remove them too."
fi
# This file removes itself last.
rm -f /usr/local/bin/david-uninstall.sh
exit 0
