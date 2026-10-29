# David und Psalm

Selbst gehosteter Musik- und Hörbuchdienst für die Familie, für Android, iPhone
und Desktop.

- **David** ist der Server (Bibliothek, Profile, Hörfortschritt).
- **Psalm** ist die App. Zusammen sind sie das Paar aus dem Namen: David und seine Psalmen.

Status: früh, aber benutzbar auf Linux und im Android-Emulator.

- **David (Server):** Rollen, Altersfreigabe, Geräte-Grenzen und Wochenplan; Hörbücher und Musik
  (Autor und Sprecher, Interpret und Album); Import mit automatischer Suche nach neuen Dateien, Upload von
  ZIP/TAR/RAR/7z mit Warteschlange, Tags und Cover im Browser bearbeiten, Löschen; Kopplung der Apps per Code/QR,
  TLS mit festgehaltenem Fingerabdruck, Ankündigung per mDNS; Verwaltungsoberfläche im Browser (Deutsch/Englisch)
  mit Suche und Vervollständigung in den Medienlisten; Tags in voller Breite (alle Felder, die `lofty` kennt).
  Läuft als fester Systembenutzer `david` (Paket legt ihn an); `/healthz` für die Überwachung; Zertifikat mit allen
  Adressen des Rechners (`david renew-cert` erneuert es, danach alle Geräte neu koppeln).
- **MCP für ein entferntes LLM:** `david run --mcp` öffnet `/mcp`; Zugang nur mit Token (Einstellungen, Tab
  „LLM-Zugang“), lesend oder schreibend. Werkzeuge: Alben und Tags lesen, `find_problems` (fehlende oder
  widersprüchliche Tags), `set_tags` mit `dry_run`, Cover aus dem Ordnerbild einbetten. Jede Änderung wird mit altem
  und neuem Wert protokolliert (`docs/releasing.md`, Abschnitt MCP).
- **Psalm (App):** Bibliothek, Kopplung (Code einfügen, QR aus Bild), Wiedergabe mit Tempo ohne Tonhöhenänderung,
  Download für Hören ohne Server, Fortschritt (wird beim Start wieder aufgenommen), Aktualisierung der Bibliothek.
  Ton über PipeWire (Linux) und `just_audio` (Android).
- **Offen:** Offline-Start der App ohne Server (die Bibliothek wird noch nicht lokal gespeichert), Musik in der App,
  Hintergrundwiedergabe und Sperrbildschirm, iPhone, Windows, signierte Pakete (siehe `docs/releasing.md`).

## Ziel

- Eigene Bibliothek (Hörbücher und Musik) auf eigenem Server
- Apps für Android und iPhone, dazu Desktop (Linux, Windows)
- Hörposition je Titel merken, Download, Hintergrundwiedergabe, Sperrbildschirm
- Keine Abhängigkeit von einem Anbieter-Konto

## Geplante Architektur

| Teil | Technik | Stand |
|---|---|---|
| App | Flutter, Rust-Kern über `flutter_rust_bridge` | Oberfläche in `app/` mit Platzhalterdaten, ohne Ton |
| Server | Rust (`axum`, `lofty`, SeaORM mit SQLite) | läuft (Debian-/RPM-Paket, systemd); der Server `PDC` der Familie ist der erste Zielhost |
| Schnittstelle | eigene REST-Schnittstelle (`/api/v1`, Token je Gerät) und MCP für LLMs | läuft; eine Subsonic-kompatible API ist nicht begonnen |

Das System ist für eine Familie gedacht: **höchstens 150 Profile**, Datenbank ist
**SQLite** (viel Lesen, wenig Schreiben, kein eigener Datenbankserver). Jedes
Profil hat eigene Hörpositionen und Favoriten, die Bibliothek ist gemeinsam. Der
Fortschritt liegt auf dem Server.

Zugriff von unterwegs läuft über **WireGuard im Userspace**, in Server und App
eingebaut (kein VPN-Eintrag im Betriebssystem). Jedes Gerät hat einen eigenen
Schlüssel und wird mit einem Einmal-Code eingerichtet.

Flutter mit Rust-Kern gilt als gemeinsame Basis, damit Server und Client auch
unter Windows laufen. Wiedergabe im Hintergrund, Sperrbildschirm und Auto
(Android Auto, CarPlay) hängen an den Betriebssystemschnittstellen. Das ist der
Teil, der zuerst ausprobiert werden muss (`just_audio`, `audio_service`).

Als Rückfallebene bleiben fertige Server: Audiobookshelf für Hörbücher,
Navidrome für Musik.

## Plattformen

**Zuerst Linux**, dann Android. iPhone und Windows folgen später.

| Plattform | Stand |
|---|---|
| Linux (Desktop, Server) | Ton über PipeWire (`audio/`), Server `david` läuft; hier wird zuerst gebaut |
| Android | läuft im Emulator (Debug- und Release-APK, `just_audio` hinter der gemeinsamen Schnittstelle `AudioBackend`); noch nicht auf einem echten Telefon, nicht veröffentlicht |
| iPhone | später (AirPlay und CarPlay kommen vom System) |
| Windows | später; der Dienstcode des Servers ist geschrieben, aber nicht auf dem Zielsystem geprüft, nur gebaut und getestet |

## Aufbau des Repositorys

| Ordner | Inhalt |
|---|---|
| `app/` | Psalm, die Flutter-App (Linux, Android) |
| `audio/` | Ton unter Linux: Rust-Engine über PipeWire (Laden, Abspielen, Pause, Springen, Lautstärke, Position), C-Schnittstelle für Flutter, Prüfskripte gegen eine virtuelle Senke |
| `server/` | David, der Server (Programm `david`): SQLite mit SeaORM, Profile, Fortschritt, Bibliotheksscan, Dienstbetrieb (systemd, Windows-Dienst), Tests und Zapp-Test |
| `docs/` | Specs auf Englisch: Datenmodell, Nebenläufigkeit, Audio (PipeWire), Entdeckung/Profile/Rollen, Verwaltungsoberfläche, App-Schnittstelle, Bibel, Tags/ID3 und Genres (`docs/spec-tags-id3.md`), CD-Rippen, Veröffentlichen |
| `scripts/` | Pakete bauen (`package.sh`, `package-app.sh`), Prüfsummen (`checksums.sh`) |
| `.github/workflows/` | CI: Tests und Pakete (.deb, .rpm, .apk), siehe `docs/releasing.md` |

Kommentare im Quellcode sind immer englisch.

## Sprachen

Alle Texte der Oberfläche kommen aus ARB-Dateien (`app/lib/l10n/`), nichts ist
fest im Code. **Englisch ist die erste Sprache**: Es ist die Vorlage
(`app_en.arb`) und der Rückfall, wenn die Systemsprache nicht unterstützt wird.
Deutsch ist die zweite Sprache (`app_de.arb`).

Neuer Text: Schlüssel in `app_en.arb` anlegen (mit Platzhalter-Metadaten), dann in
jeder weiteren Sprache übersetzen und `flutter gen-l10n` ausführen. Zahlen und
Plurale laufen über ICU-Syntax in der ARB-Datei, nicht über Stringbau im Code.

## Entwicklung

- Flutter ist systemweit installiert (`/opt/flutter`, Kanal stable).
- iOS-Builds gehen nur auf einem Mac.
- Für den Emulator zeigt `adb reverse tcp:8484 tcp:8484` den Server-Port des Rechners auf `127.0.0.1` im Emulator.
- Pakete und Veröffentlichung: `docs/releasing.md`.
- Server prüfen: `cd server && cargo test && cargo clippy --all-targets` (einige Tests brauchen `ffmpeg` und
  überspringen sich sonst).

## Namenswahl

"Psalm" ist im App Store und bei Google Play vielfach durch Bibel-Apps belegt
(Stand der Websuche vom 2026-10-27, kein Anspruch auf Vollständigkeit). Für den
privaten Gebrauch egal, vor einer Veröffentlichung Name, Marke und Domain prüfen.

## Lizenz

GNU Affero General Public License, Version 3 oder neuer (AGPL-3.0-or-later), siehe `LICENSE`.
