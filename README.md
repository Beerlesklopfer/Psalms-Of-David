# David und Psalms

Psalms, das Mediacenter deiner Familie. Selbst gehosteter Medienserver für die Familie: Hörbücher, Musik, Podcasts und Radio, mit Apps für Linux, Android,
iPhone/iPad und macOS.

- **David** ist der Server (Bibliothek, Profile, Hörfortschritt, Verwaltung im Browser).
- **Psalms** ist die App (Programm- und Paketname `psalm`). Zusammen sind sie das Paar aus dem Namen: David und seine Psalmen.

**Für alle, die nur zuhören:** Wer David betreut, gibt dir einen Code. Mit dem verbindest du dieses Gerät einmal, danach hörst du alles,
was für dich freigegeben ist. Die Hörbücher, die Musik, die Podcasts und das Radio liegen zunächst nicht in der App, sondern auf dem
Computer zu Hause, auf dem David läuft; das entlastet den Speicher deines Geräts. Möchtest du Musik mitnehmen, lädst du nur herunter,
was du gerade brauchst.

**Status:** früh, aber im Familienbetrieb benutzbar. Erste Vorabversion: [Preview 1](https://github.com/Beerlesklopfer/Psalms-Of-David/releases/tag/preview-1)
(lokal gebaut, unsigniert, zum Ausprobieren).

## Was es kann

- **David (Server)**
  - Bibliothek: Hörbücher (Autor und Sprecher) und Musik (Interpret, Album, Genre), Podcasts (RSS) und Radiosender,
    Wiedergabelisten je Profil, Labels, Besitzer je Album, Ausblenden je Profil.
  - Familie: Rollen (Admin, Mitglied, Kind, Gast), Altersfreigabe, Freigaben je Profil oder Rolle, Geräte-Grenzen,
    Wochenplan mit Hörzeit, Statistik der Hörzeit.
  - Import: automatische Suche nach neuen Dateien, Upload von ZIP/TAR/RAR/7z mit Warteschlange, Audio-CDs rippen im
    Laufwerk des Servers mit Daten von MusicBrainz, Dubletten und Quarantäne, Umwandlung in Formate, die jedes Gerät
    abspielt (`docs/spec-transcoding.md`).
  - Tags und Cover im Browser bearbeiten, in voller Breite (alle Felder, die `lofty` kennt); Bibelstellen auflösen;
    Barcode (EAN/GTIN) zu Album.
  - Kopplung der Apps per Code/QR, TLS mit festgehaltenem Fingerabdruck (oder ein eigenes Zertifikat, siehe `FAQ.md`),
    Ankündigung per mDNS.
  - Verwaltungsoberfläche im Browser (Deutsch/Englisch) mit Suche und Vervollständigung.
  - Betrieb: Paket für Debian/RPM mit systemd und festem Systembenutzer `david`, macOS mit launchd, `/healthz` für die
    Überwachung. Rechtlich unklare Funktionen sind aus und werden nur in `/etc/david.toml` (`[features]`) eingeschaltet.
- **MCP für ein LLM:** `david run --mcp` öffnet `/mcp`, Zugang nur mit Token (Einstellungen, Tab „LLM-Zugang“), lesend
  oder schreibend. Rund 50 Werkzeuge: Bibliothek und Tags lesen, `find_problems`, `set_tags` mit `dry_run`, Cover,
  Genres, Alben verschieben und zusammenführen, Dubletten, Wiedergabelisten, Podcasts, Sender, CD-Laufwerk. Jede
  Änderung wird mit altem und neuem Wert protokolliert (`docs/releasing.md`, Abschnitt MCP).
- **Psalms (App)**
  - Bibliothek nach Musik, Hörbüchern, Podcasts, Radio und Wiedergabelisten, Suche, Labels, Ausblenden.
  - Kopplung (Code einfügen, QR mit der Kamera oder aus einem Bild).
  - Wiedergabe lückenlos, mit Tempo ohne Tonhöhenänderung; Fortschritt auf dem Server, wird beim Start wieder aufgenommen.
  - Hintergrundwiedergabe und Sperrbildschirm (Android, iOS über `audio_service`), MPRIS unter Linux.
  - Download zum Hören ohne Server; die Bibliothek wird auf dem Gerät gespeichert, die Offline-Ansicht geht auch
    nach einem Neustart ohne Server.
  - CD-Import vom Gerät aus, Barcode scannen, Hörzeit-Diagramm.
  - Siri spielt aus Psalms (iOS, erste Stufe, `docs/spec-voice.md`).
  - Ton über die eigene Engine mit PipeWire (Linux, `audio/`) und `just_audio` (Android, iOS) hinter der gemeinsamen
    Schnittstelle `AudioBackend`.

## Offen

- **Unterwegs:** Zugriff über WireGuard (siehe Architektur) ist geplant; beim Koppeln wird schon ein Schlüssel
  übergeben, der Tunnel selbst fehlt noch.
- **Auto:** Hintergrund und Sperrbildschirm laufen; der Medienbaum für Android Auto und CarPlay ist nicht gebaut
  (`docs/spec-car.md`).
- **Plattformen:** Android auf einem echten Telefon, Windows (siehe unten).
- **Pakete:** signierte Pakete und Releases (`docs/releasing.md`).
- **Weitere Medien:** E-Books und Videos zeigt die App als Kategorie, David verwaltet sie noch nicht
  (`docs/spec-ebooks.md`, Entwurf).
- **Konzepte ohne Code:** Vorlesen in den Stimmen der Eltern (`docs/spec-read-aloud.md`), Predigten als Text
  (`docs/spec-transcription.md`), Abraham Box (`docs/spec-abraham-box.md`, Entwurf), Foto-Abgleich
  (`docs/spec-photo-sync.md`). Was als Nächstes kommt, steht in `docs/backlog.md`.
- Eine Subsonic-kompatible API ist nicht begonnen.

## Schnellstart (Server unter Linux)

Paket aus dem Release installieren (`.deb` oder `.rpm`; für das Entpacken hochgeladener Archive `7zip` bzw.
`p7zip-full`), dann vor dem ersten Start den ersten Administrator anlegen:

    sudo systemctl stop david
    sudo -u david env DAVID_DATA_DIR=/var/lib/david /usr/bin/david init --name Papa     # fragt nach einer PIN
    sudo systemctl start david

Die Verwaltung liegt unter `https://<rechner>:8484/admin` (Port 8484 in der Firewall öffnen). Dort die Import-Ordner
einstellen, Profile anlegen und für jedes Gerät einen Kopplungscode erzeugen, den die App per QR oder Code annimmt.
Einzelheiten (Rechte auf Ordnern, Adressen, macOS, eigenes Zertifikat): `docs/releasing.md` und `FAQ.md`.

## Architektur

| Teil | Technik | Stand |
|---|---|---|
| App | Flutter; Ton-Engine in Rust (`audio/`, `libpsalm_audio`) über `dart:ffi` und eine C-Schnittstelle (`docs/spec-ffi-architecture.md`) | läuft (Linux, Android, iOS, macOS) |
| Server | Rust (`axum`, `lofty`, SeaORM mit SQLite) | läuft (Debian-/RPM-Paket, systemd; macOS); der Server `PDC` der Familie ist der erste Zielhost |
| Schnittstelle | eigene REST-Schnittstelle (`/api/v1`, Token je Gerät), WebSocket für Ereignisse, MCP für LLMs | läuft |

Das System ist für eine Familie gedacht: **höchstens 150 Profile**, Datenbank ist **SQLite** (viel Lesen, wenig
Schreiben, kein eigener Datenbankserver). Jedes Profil hat eigene Hörpositionen und Favoriten, die Bibliothek ist
gemeinsam. Der Fortschritt liegt auf dem Server.

Zugriff von unterwegs soll über **WireGuard im Userspace** laufen, in Server und App eingebaut (kein VPN-Eintrag im
Betriebssystem). Jedes Gerät hat einen eigenen Schlüssel und wird mit einem Einmal-Code eingerichtet. Gebaut ist bisher
nur die Übergabe des Schlüssels beim Koppeln.

Als Rückfallebene bleiben fertige Server: Audiobookshelf für Hörbücher, Navidrome für Musik.

## Plattformen

| Plattform | Stand |
|---|---|
| Linux (Desktop, Server) | Ton über PipeWire (`audio/`), Server `david` läuft; hier wird zuerst gebaut |
| Android | läuft im Emulator (Debug- und Release-APK); noch nicht auf einem echten Telefon geprüft, nicht veröffentlicht |
| iPhone, iPad | zwei Apps (zwei Bundle-IDs), Testbuilds über TestFlight; auf einem Gerät ausprobiert (Siri, `docs/spec-voice.md`) |
| macOS | App als `.zip`/`.dmg` aus der CI; der Server läuft mit launchd (`docs/releasing.md`) |
| Windows | später; der Dienstcode des Servers ist geschrieben, gebaut und getestet, aber nicht auf dem Zielsystem geprüft |

## Aufbau des Repositorys

| Ordner | Inhalt |
|---|---|
| `app/` | Psalms, die Flutter-App (Linux, Android, iOS, macOS, Windows) |
| `audio/` | Ton-Engine in Rust: PipeWire (Laden, Abspielen, Pause, Springen, Lautstärke, Position), C-Schnittstelle für Flutter, Prüfskripte gegen eine virtuelle Senke |
| `server/` | David, der Server (Programm `david`): SQLite mit SeaORM, Profile, Fortschritt, Bibliotheksscan, CD-Rippen (`server/cdrip/`), Dienstbetrieb (systemd, launchd, Windows-Dienst), Tests |
| `docs/` | Specs auf Englisch (Datenmodell, Nebenläufigkeit, Audio, Profile und Rollen, Verwaltung, App-Schnittstelle, Tags, CD, Wiedergabelisten, Sprachsteuerung …), `releasing.md`, `backlog.md` |
| `site/` | Website von Psalms und David auf GitHub Pages (Datenschutz, Support, Download, Impressum) |
| `scripts/` | Pakete bauen (`package.sh`, `package-app.sh`), Prüfsummen (`checksums.sh`) |
| `.github/workflows/` | CI: Tests und Pakete (.deb, .rpm, .tar.gz, .apk, macOS, iOS), Website; siehe `docs/releasing.md` |

Kommentare im Quellcode sind immer englisch.

## Sprachen

Alle Texte der Oberfläche kommen aus ARB-Dateien (`app/lib/l10n/`), nichts ist fest im Code. **Englisch ist die erste
Sprache**: Es ist die Vorlage (`app_en.arb`) und der Rückfall, wenn die Systemsprache nicht unterstützt wird. Deutsch
ist die zweite Sprache (`app_de.arb`).

Neuer Text: Schlüssel in `app_en.arb` anlegen (mit Platzhalter-Metadaten), dann in jeder weiteren Sprache übersetzen
und `flutter gen-l10n` ausführen. Zahlen und Plurale laufen über ICU-Syntax in der ARB-Datei, nicht über Stringbau im
Code.

## Entwicklung

- Flutter ist systemweit installiert (`/opt/flutter`, Kanal stable).
- iOS- und macOS-Builds gehen nur auf einem Mac.
- Für den Emulator zeigt `adb reverse tcp:8484 tcp:8484` den Server-Port des Rechners auf `127.0.0.1` im Emulator.
- Pakete und Veröffentlichung: `docs/releasing.md`.
- Server prüfen: `cd server && cargo test && cargo clippy --all-targets`. Zum Bauen wird `libacl` gebraucht (Debian:
  `libacl1-dev`). Einige Tests brauchen `ffmpeg` und überspringen sich sonst; die Tests zu mDNS und IPv6 brauchen ein
  Netz mit Multicast und IPv6, und ein Ordnertest erwartet, nicht als root zu laufen.

## Namenswahl

"Psalms" ist im App Store und bei Google Play vielfach durch Bibel-Apps belegt (Stand der Websuche vom 2026-10-27,
kein Anspruch auf Vollständigkeit). Für den privaten Gebrauch egal, vor einer Veröffentlichung Name, Marke und Domain
prüfen.

## Lizenz

GNU Affero General Public License, Version 3 oder neuer (AGPL-3.0-or-later), siehe `LICENSE`.
