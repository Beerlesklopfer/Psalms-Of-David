// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for German (`de`).
class AppLocalizationsDe extends AppLocalizations {
  AppLocalizationsDe([String locale = 'de']) : super(locale);

  @override
  String get appTitle => 'Psalm';

  @override
  String get navHome => 'Start';

  @override
  String get navSearch => 'Suche';

  @override
  String get navLibrary => 'Bibliothek';

  @override
  String get greetingMorning => 'Guten Morgen';

  @override
  String get greetingDay => 'Guten Tag';

  @override
  String get greetingEvening => 'Guten Abend';

  @override
  String get sectionContinue => 'Weiterhören';

  @override
  String get sectionYourBooks => 'Deine Hörbücher';

  @override
  String get sectionAuthors => 'Autoren';

  @override
  String get sectionChapters => 'Kapitel';

  @override
  String discLabel(int number) {
    return 'CD $number';
  }

  @override
  String discTrack(int disc, String title) {
    return 'CD $disc · $title';
  }

  @override
  String bookMeta(int discs, int tracks, String duration) {
    String _temp0 = intl.Intl.pluralLogic(
      discs,
      locale: localeName,
      other: '$discs CDs',
      one: '1 CD',
    );
    String _temp1 = intl.Intl.pluralLogic(
      tracks,
      locale: localeName,
      other: '$tracks Tracks',
      one: '1 Track',
    );
    return '$_temp0 · $_temp1 · $duration';
  }

  @override
  String durationHoursMinutes(int hours, int minutes) {
    return '$hours Std. $minutes Min.';
  }

  @override
  String durationMinutes(int minutes) {
    return '$minutes Min.';
  }

  @override
  String percentListened(int percent) {
    return '$percent % gehört';
  }

  @override
  String get actionListen => 'Anhören';

  @override
  String get actionContinue => 'Weiterhören';

  @override
  String get tooltipPlay => 'Wiedergabe';

  @override
  String get tooltipPause => 'Pause';

  @override
  String get tooltipClose => 'Schließen';

  @override
  String get tooltipClear => 'Löschen';

  @override
  String get tooltipPrevious => 'Vorheriger Track';

  @override
  String get tooltipNext => 'Nächster Track';

  @override
  String get tooltipBack10 => '10 Sekunden zurück';

  @override
  String get tooltipForward30 => '30 Sekunden vor';

  @override
  String get tooltipDownloadUnavailable =>
      'Herunterladen (noch nicht verfügbar)';

  @override
  String get nothingSelected => 'Nichts ausgewählt';

  @override
  String get searchHint => 'Titel oder Autor';

  @override
  String get nothingFound => 'Nichts gefunden';

  @override
  String get filterBooks => 'Hörbücher';

  @override
  String get filterMusic => 'Musik';

  @override
  String get noMusicYet => 'Noch keine Musik in der Bibliothek';

  @override
  String get connectTitle => 'Mit deinem Server verbinden';

  @override
  String get connectIntro =>
      'Öffne die Verwaltungsseite deines Servers, erzeuge einen Kopplungscode und füge die Adresse darunter hier ein.';

  @override
  String get pairingAddressLabel => 'Kopplungsadresse';

  @override
  String get deviceNameLabel => 'Name dieses Geräts';

  @override
  String get connectButton => 'Verbinden';

  @override
  String get retryButton => 'Erneut versuchen';

  @override
  String get useDemoButton => 'Mit Demodaten ausprobieren';

  @override
  String get connectErrorAddress => 'Das ist keine Kopplungsadresse.';

  @override
  String get connectErrorCode =>
      'Der Code ist ungültig, schon benutzt oder abgelaufen. Erzeuge einen neuen.';

  @override
  String get connectErrorLimit =>
      'Dieser Benutzer hat die Grenze für Geräte erreicht.';

  @override
  String get connectErrorNetwork => 'Der Server ist nicht erreichbar.';

  @override
  String get connectErrorCertificate =>
      'Der Server zeigt ein anderes Zertifikat als im Code.';

  @override
  String get connectErrorOther => 'Das Verbinden ist fehlgeschlagen.';

  @override
  String openPassage(String text) {
    return 'Stelle „$text“ aufschlagen';
  }

  @override
  String get passageNotFound =>
      'Keine Aufnahme enthält diese Stelle für dich im Moment.';

  @override
  String get loadingFile => 'Lädt …';

  @override
  String get pasteButton => 'Einfügen';

  @override
  String get qrButton => 'QR-Code aus einem Bild lesen';

  @override
  String get qrNotFound =>
      'In diesem Bild wurde kein QR-Code gefunden. Versuche ein schärferes oder größeres.';

  @override
  String get qrChooseTitle => 'Bild mit dem QR-Code wählen';

  @override
  String bookPlayed(String time) {
    return 'Buch: $time gehört';
  }

  @override
  String bookTotal(String time) {
    return 'von $time';
  }

  @override
  String get navSettings => 'Einstellungen';

  @override
  String get navAbout => 'Über';

  @override
  String get settingsServer => 'Server';

  @override
  String get settingsDemo =>
      'Das ist die Demo-Bibliothek. Es ist kein Server verbunden.';

  @override
  String get settingsAddress => 'Adresse';

  @override
  String get settingsDevice => 'Dieses Gerät';

  @override
  String get settingsDisconnect => 'Trennen';

  @override
  String get settingsDisconnectHint =>
      'Vergisst die Kopplung mit dem Server auf diesem Gerät. Zum erneuten Verbinden brauchst du einen neuen Kopplungscode.';

  @override
  String get cancel => 'Abbrechen';

  @override
  String aboutVersion(String version) {
    return 'Version $version';
  }

  @override
  String get aboutDescription =>
      'Psalm spielt Hörbücher, Musik und mehr von deinem eigenen David-Server, für die ganze Familie.';

  @override
  String get aboutLicense =>
      'Freie Software unter der GNU Affero General Public License, Version 3 oder neuer.';

  @override
  String get aboutLicenses => 'Open-Source-Lizenzen';

  @override
  String get tooltipSpeed => 'Geschwindigkeit';

  @override
  String get aboutLicenseTitle => 'Lizenz';

  @override
  String get aboutScripture => 'Bibelstellen';

  @override
  String get aboutFamily => 'Familie';

  @override
  String get connectNeedsDavid =>
      'Psalm ist nur der Player. Es braucht einen David-Server, auf dem deine Hörbücher, deine Musik und die Profile der Familie liegen. Ohne einen gibt es in Psalm nichts zu hören.';

  @override
  String get aboutDavidTitle => 'Psalm braucht einen David-Server';

  @override
  String get aboutDavidText =>
      'Psalm bringt selbst keine Inhalte mit. Es spielt, was ein David-Server anbietet: Hörbücher, Musik und mehr aus deiner eigenen Bibliothek, mit einem Profil für jedes Familienmitglied. David ist freie Software, die du selbst betreibst, auf einem Heimserver oder einem kleinen Rechner. Wer ihn betreibt, erzeugt auf der Verwaltungsseite einen Kopplungscode; den gibst du hier einmal ein. Ohne David-Server kannst du nur die Demodaten ausprobieren.';

  @override
  String get aboutDavidSteps =>
      '1. David-Server betreiben.  2. Seine Verwaltungsseite öffnen und „Gerät koppeln“ wählen.  3. Den Code in Psalm scannen oder einfügen.';

  @override
  String get tooltipRefresh => 'Bibliothek aktualisieren';

  @override
  String get tooltipDownload => 'Herunterladen, um ohne Server zu hören';

  @override
  String get tooltipDownloadCancel => 'Download anhalten';

  @override
  String get tooltipDownloaded => 'Auf diesem Gerät. Tippen zum Entfernen.';

  @override
  String downloadedCount(int done, int total) {
    return '$done von $total';
  }

  @override
  String get downloadRemoveTitle => 'Download entfernen?';

  @override
  String get downloadRemoveText =>
      'Die Dateien werden von diesem Gerät gelöscht. Das Buch bleibt auf dem Server und kann wieder heruntergeladen werden.';

  @override
  String get downloadRemove => 'Entfernen';

  @override
  String get aboutStorage => 'Speicher auf diesem Gerät';

  @override
  String get aboutStorageNone => 'Keine Downloads';

  @override
  String aboutStorageUsed(String size, int count) {
    return '$size in $count heruntergeladenen Teilen';
  }

  @override
  String get aboutStorageRemoveTitle => 'Alle Downloads entfernen?';

  @override
  String get craTitle => 'Sicherheitsinformationen (EU Cyber Resilience Act)';

  @override
  String get craManufacturer => 'Hersteller';

  @override
  String get craVersion => 'Version und Lizenz';

  @override
  String get craContact => 'Sicherheitsprobleme melden an';

  @override
  String get craSupport => 'Sicherheitsupdates';

  @override
  String get craSupportOpen =>
      'Kommen mit neuen Versionen. Das Ende des Support-Zeitraums ist noch nicht festgelegt.';

  @override
  String craSupportUntil(String date) {
    return 'Bis $date';
  }

  @override
  String get craData => 'Daten und Netzwerk';

  @override
  String get craDataText =>
      'Psalm spricht nur mit dem David-Server, mit dem es gekoppelt ist, über eine Verbindung, die das Zertifikat des Servers prüft. Es gibt keine Analyse und keine Werbung. Die Kopplung und die heruntergeladenen Dateien bleiben auf diesem Gerät.';

  @override
  String get craComponents => 'Software-Komponenten';

  @override
  String get craComponentsText =>
      'Die Liste aller Komponenten mit ihren Lizenzen.';

  @override
  String get aboutSource => 'Quellcode';

  @override
  String get filterVideos => 'Videos';

  @override
  String get filterEbooks => 'E-Books';

  @override
  String get comingSoonTitle => 'Noch nicht verfügbar';

  @override
  String get comingSoonVideos =>
      'Videos sind geplant. David verwaltet sie noch nicht, deshalb gibt es hier nichts zu zeigen.';

  @override
  String get comingSoonEbooks =>
      'E-Books und PDF-Dateien sind geplant. David verwaltet sie noch nicht, deshalb gibt es hier nichts zu zeigen.';

  @override
  String get noAudiobooksYet => 'Noch keine Hörbücher in der Bibliothek';

  @override
  String readBy(String name) {
    return 'Gelesen von $name';
  }

  @override
  String get filterAll => 'Alle';

  @override
  String get sectionArtists => 'Interpreten';

  @override
  String get sectionAuthorsAndArtists => 'Autoren und Interpreten';

  @override
  String artistLabel(String name) {
    return 'Künstler: $name';
  }

  @override
  String get sectionYourMusic => 'Deine Musik';

  @override
  String get craEmail => 'Kontakt';

  @override
  String get craWebsite => 'Webseite';
}
