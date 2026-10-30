# Frequently asked questions

## How do I use a certificate from Let's Encrypt (or another certificate authority)?

At its first start David makes a certificate of its own. When a device is paired it remembers the
fingerprint of that certificate and trusts only this server. That is the default and needs no setting.

A certificate from Let's Encrypt is renewed about every 60 days, and with each renewal the fingerprint
changes. A device that pins the fingerprint would then refuse the server. With such a certificate the devices
have to check the name instead, and the server needs a host name, not only an IP address
(`david.example.org`, not `192.168.0.2`). The pairing code then carries the name and the mark `ca=1`
instead of a fingerprint, and the app (Psalms) checks the name against the certificate and the trust of the system.

The files are named in `/etc/david.toml` (`tls_cert`, `tls_key`, `tls_host`), or in the settings of the pages
(Settings > Values > Certificate), or by `--tls-cert`, `--tls-key`, `--tls-host` and `DAVID_TLS_*`. A renewed
certificate is loaded without a restart.

### Principle: access runs through a group

The server sets no rights on files that do not belong to it: no owner, no group, no ACLs. It reads what its
service user may read, and says so in the browser (a warning) and in the log when it cannot read a file. Which
files those are is the administrator's decision, and who may read them is regulated by a **group**: the
service user (`david`) is a member of that group, and the group may read the certificates. On Debian the group
is usually called `ssl-cert`; the name below stands in one place only and can be another.

### Setting it up

1. Add the service user to the group and restart the service afterwards (the groups of a running process
   change only at its start): `usermod -aG <group> david`, `systemctl restart david`.
2. Let the group enter the folders `archive` and `live` and read the key (`certbot` makes them readable for
   `root` only).
3. Put the same into a `certbot` deploy hook, so that a renewal does not reset the rights.

The hook (`/etc/letsencrypt/renewal-hooks/deploy/10-ssl-group.sh`, make it executable). It takes the place and
the name from `certbot` itself (`$RENEWED_LINEAGE`); only the group stands at the top as a line of its own:

```sh
#!/bin/sh
set -e
group=ssl-cert
live=$(dirname "$RENEWED_LINEAGE")
base=$(dirname "$live")
name=$(basename "$RENEWED_LINEAGE")
chgrp "$group" "$base/archive" "$live"
chmod 750 "$base/archive" "$live"
chgrp -R "$group" "$base/archive/$name"
chmod 640 "$base/archive/$name"/privkey*.pem
```

Whoever is in the group can then read all the keys under that folder. That is the usual consequence of this group.

### The commands in `fish`

The hook is an `sh` script (shebang `#!/bin/sh`) and runs in any environment. The commands around it differ in
`fish`: a variable is set with `set NAME value` instead of `NAME=value`, and `fish` refuses a `*` in a folder the
user may not read (so `sudo sh -c "…"`).

```fish
# Group, folder and name of the certificate stand here and nowhere else
set G ssl-cert
set L /etc/letsencrypt
set N example.org

# 1. The service user into the group
sudo usermod -aG $G david

# 2. The group may enter the folders and read the key
sudo chgrp $G $L/archive $L/live
sudo chmod 750 $L/archive $L/live
sudo chgrp -R $G $L/archive/$N
sudo sh -c "chmod 640 $L/archive/$N/privkey*.pem"

# 3. The hook, so that a renewal does not reset it
set hook $L/renewal-hooks/deploy/10-ssl-group.sh
sudoedit $hook            # paste the sh script from above, save
sudo chmod +x $hook

# Restart, so that the service has the group
sudo systemctl restart david

# Check: can the service read the certificate and the key?
sudo -u david head -c 27 $L/live/$N/fullchain.pem
sudo -u david head -c 27 $L/live/$N/privkey.pem

# The hook on a renewal, without renewing anything:
sudo certbot renew --dry-run
```

The two checks should print `-----BEGIN CERTIFICATE-----` and `-----BEGIN PRIVATE KEY-----` (`EC PRIVATE KEY`
for some key types). If `Permission denied` stands there, `david` lacks the group (service restarted?) or the
group may not enter one of the folders.

To undo it: `sudo gpasswd -d david $G`, then `chgrp root` and `chmod 700` on `archive`, `live` and
`archive/$N`, and `chmod 600` on the `privkey*.pem`.

## How does David read audio CDs in the drive of the server?

The CD drive sits at the server, so the server reads the disc (Settings > CD import; the apps only show the state
and start the import). The service user `david` has to be allowed to open the drive. As with the certificates, the
server sets **no rights on files that do not belong to it**: it reads what its service user may read, and says so
in the browser (a warning at the top of the tab "CD import", and in the drive panel) when it may not use the drive.
Who may use the drive is regulated by a **group**; the administrator makes the service user a member. On Debian the
drive (`/dev/sr0`) belongs to the group `cdrom`; the name can be another on your system (`ls -l /dev/sr0`).

### What the unit allows

The packaged unit (`david.service`) has no `PrivateDevices=` and no `DeviceAllow=`, so `/dev/sr*` is visible and the
group decides. The service reads the kernel's drive events (netlink) when "Detect CD drives with udev" is on; the
unit does not restrict address families. If you harden the unit yourself with a drop-in (`systemctl edit david`),
keep the device and the socket reachable:

```
[Service]
# Only needed with PrivateDevices=yes or DevicePolicy=closed/strict:
DeviceAllow=block-sr rw
# Only needed with RestrictAddressFamilies=: the drive events come over AF_NETLINK.
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK
```

### Setting it up

1. Add the service user to the group: `sudo usermod -aG cdrom david`.
2. Name the group in `/etc/david.toml` (the service starts as root and takes the groups of the list for the
   service user; the key `groups` is the same one that is used for the certificates): `groups = ["cdrom"]`
   (add to the list if there is one already).
3. Restart the service (the groups of a running process change only at its start): `sudo systemctl restart david`.
4. Check that the service user may read the drive: `sudo -u david head -c 4 /dev/sr0` (with a disc in; no
   `Permission denied`; an error like "No medium found" without a disc is fine).
5. In the settings, tab "CD import": the setting "Drive to use" stays empty to use every drive the server finds, or
   names one (`/dev/sr0`). "Detect CD drives with udev" makes the server notice the opening and closing of the tray
   and a new disc through kernel events; without it the drive is looked at every few seconds.

### The commands in `fish`

```fish
# The group of the drive on this machine (look at the name; it is "cdrom" on Debian):
ls -l /dev/sr0

set G cdrom
sudo usermod -aG $G david
grep -q 'groups' /etc/david.toml; or echo 'groups = ["'$G'"]' | sudo tee -a /etc/david.toml
sudo systemctl restart david

# Check: can the service read the drive?
sudo -u david head -c 4 /dev/sr0
```

If `Permission denied` stands there, `david` lacks the group (service restarted?). To undo it: `sudo gpasswd -d david cdrom`
and remove the entry from `groups` in `/etc/david.toml`. The server never opens or closes the tray by itself except
after an import you asked to eject; it opens the tray only when a disc is in and the tray is shut, and never closes it.

### The drive pumps: buffer, read speed, and the kernel asking the drive

A drive that reads at full speed and then has to wait slows down and speeds up again, and every pause costs time. Three things act on it, all in Settings > CD import:

1. **The buffer.** The server reads into a ring buffer in memory without waiting for the encoder and goes on over the border of the next track. "Buffer between the drive and the encoder" is the size: **0 (automatic)** makes it big enough for the longest chosen track plus 10 seconds, so a whole track fits; a number gives seconds of audio (2 to 900). "The most memory the buffer may take" (default 256 MB) caps both. One second of audio is 176 kB; a 20 minute track needs about 213 MB, a 5 minute track about 55 MB, and only one disc is read at a time. While a CD is read the panel shows "Buffer 41 s of 230 s (18 %)" and, if the reader had to wait, how often and how long. A buffer that is mostly full means the encoder or the disk is the brake, a buffer that is mostly empty means the drive is. After the rip the log of the tab says in one line how long the drive read and how long the reader waited.
2. **The read speed.** "Read speed during a rip", default 8 (that is 8 x 176 kB/s; 0 leaves it to the drive). A steady moderate speed lets the drive run evenly; a scratched disc often reads better at 4 or lower. The drive gets its own speed back when the rip ends.
3. **The kernel and the drive.** While a rip runs the server asks the drive nothing (no status, no table of contents), and the web panel and the apps read only what the server already knows. The package installs a udev rule (`/usr/lib/udev/rules.d/61-david-cdrom.rules`) that makes the kernel ask an optical drive for a change of disc only every 10 minutes instead of every few seconds; with that the drive reads evenly. The price is that a disc is noticed late by the kernel, so keep "Detect CD drives with udev" switched off (the server then looks at the drive itself every "cd_poll_seconds"). The file explains how to change the time or switch the rule off. If the drive still pumps, look at what else asks it: a desktop that polls the drive (udisks, a file manager, a media player) or `udev` rules; the setting "Detect CD drives with udev" makes the server itself rely on the kernel's events instead of polling. "Read every track twice and compare" makes the rip about twice as long (a second pass over the disc); it can be switched off for clean discs.

### Why was a rip not resumed? (the log of a development build)

What the server decides when it finds the folder of an interrupted rip is written to the log only by a **development build** (`cargo build` or `cargo run` without `--release`); a release build, which is what the packages contain, has none of these lines. Start the development build with the level `debug` (it shows David's own code only; the libraries, such as the database queries, are logged only at `trace`) (`--log debug`, `DAVID_LOG=debug`, `log = "debug"` in `/etc/david.toml`, or the setting "Log level" in the pages). The lines start with `resume:`: "an interrupted rip of this disc was found" (with how many tracks are in the manifest and how many are taken over), "a track of the interrupted rip cannot be taken over and is read again" with the reason (`missing`, `size`, `not_on_disc`, `unreadable` or `length`), "an interrupted rip does not fit this start" (another disc, other settings or another choice of tracks) and "the staging folder of this disc is removed, the rip starts from the beginning". The measurement after a rip ("rip measured") is in every build, at `info`.

### How safe is a rip against a restart?

A rip keeps what it has finished in `/var/lib/david/cd-import/disc-<Disc ID>/` (with a file `manifest.json` that says which tracks are done). A restart of the service, also a package update, no longer throws away an hour of reading: the service gives a track in hand up to 10 seconds to finish, writes down what is done and ends (`TimeoutStopSec=60` in the unit); the job shows "interrupted". Put the same CD in again and press "Continue" in the menu item "CD import" (next to "Import"): only the missing tracks are read; the finished ones are checked first (file, size, readable, length) and read again if they do not pass. The tracks are kept for 48 hours (setting "Keep the finished tracks of an interrupted rip for"); a change of the audio settings (format, bit rate, ...) means the CD is read again from the start, while a different choice of tracks does not: what was read and is still ticked is kept, a track that is not ticked any more is dropped (the list of tracks marks the ones that were read). If the machine itself fails (power) the tracks that were written down are kept as well; a track that was half done is read again.

### The CD is in the library already

If you put in a CD whose Disc ID an import has stored, or whose book (author, title, CD number) the library has, the panel says "This CD is in the library already" instead of offering an import, and shows what is missing or damaged. "Repair" reads only those tracks, puts a damaged file into the quarantine first (never overwrites it) and names the new files like the others of that CD. The automatic import (off by default, needs the kernel events) never imports such a CD again by itself; every case in which it did not start is in the log of the tab with the reason.
