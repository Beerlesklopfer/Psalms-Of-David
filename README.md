# David und Psalm

Selbst gehosteter Musik- und Hörbuchdienst für die Familie, für Android, iPhone
und Desktop.

- **David** ist der Server (Bibliothek, Profile, Hörfortschritt).
- **Psalm** ist die App. Zusammen sind sie das Paar aus dem Namen: David und seine Psalmen.

Status: früh. Die App ist eine Oberfläche mit Platzhalterdaten und spielt noch
keinen Ton. Der Server hat Datenbank mit Rollen, Altersfreigabe, Geräte-Grenzen und Wochenplan,
Bibliotheksscan, Dienstbetrieb und eine Verwaltungsoberfläche im Browser (Anmeldung mit PIN,
Benutzer, Bücher freigeben, Einstellungen), aber noch keine Schnittstelle für die App.

## Ziel

- Eigene Bibliothek (zunächst Hörbücher, später Musik) auf eigenem Server
- Apps für Android und iPhone, dazu Desktop (Linux, Windows)
- Hörposition je Titel merken, Offline-Download, Hintergrundwiedergabe, Sperrbildschirm
- Keine Abhängigkeit von einem Anbieter-Konto

## Geplante Architektur

| Teil | Technik | Stand |
|---|---|---|
| App | Flutter, Rust-Kern über `flutter_rust_bridge` | Oberfläche in `app/` mit Platzhalterdaten, ohne Ton |
| Server | Rust (`axum`, `lofty`, `rusqlite`) | nicht begonnen |
| Schnittstelle | Subsonic-kompatible API, damit vorhandene Apps ebenfalls funktionieren | nicht begonnen |
| Zielhost | noch offen | David ist auf keinem festen Rechner installiert |

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

**Zuerst Linux.** Android ist zurückgestellt, iPhone und Windows folgen später.

| Plattform | Stand |
|---|---|
| Linux (Desktop, Server) | Ton über PipeWire (`audio/`), Server `david` läuft; hier wird zuerst gebaut |
| Android | zurückgestellt (kein Android-SDK, kein Test); vorgesehen: `just_audio` hinter einer gemeinsamen Schnittstelle |
| iPhone | später (AirPlay und CarPlay kommen vom System) |
| Windows | später; der Dienstcode des Servers ist geschrieben, aber nicht auf dem Zielsystem geprüft, nur gebaut und getestet |

## Aufbau des Repositorys

| Ordner | Inhalt |
|---|---|
| `app/` | Flutter-App (Oberfläche mit Platzhalterdaten, noch ohne Ton) |
| `audio/` | Ton unter Linux: Rust-Engine über PipeWire (Laden, Abspielen, Pause, Springen, Lautstärke, Position), C-Schnittstelle für Flutter, Prüfskripte gegen eine virtuelle Senke |
| `server/` | David, der Server (Programm `david`): SQLite mit SeaORM, Profile, Fortschritt, Bibliotheksscan, Dienstbetrieb (systemd, Windows-Dienst), Tests und Zapp-Test |
| `docs/` | Specs auf Englisch: Datenmodell, Nebenläufigkeit, Audio (PipeWire), Entdeckung/Profile/Rollen, Verwaltungsoberfläche |

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
- Linux-Desktop ist als Ziel eingerichtet, Android-SDK fehlt noch, iOS-Builds
  gehen nur auf einem Mac.

## Namenswahl

"Psalm" ist im App Store und bei Google Play vielfach durch Bibel-Apps belegt
(Stand der Websuche vom 2026-10-27, kein Anspruch auf Vollständigkeit). Für den
privaten Gebrauch egal, vor einer Veröffentlichung Name, Marke und Domain prüfen.

## Lizenz

GNU Affero General Public License, Version 3 oder neuer (AGPL-3.0-or-later), siehe `LICENSE`.
