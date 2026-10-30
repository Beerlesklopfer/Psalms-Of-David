// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for German (`de`).
class AppLocalizationsDe extends AppLocalizations {
  AppLocalizationsDe([String locale = 'de']) : super(locale);

  @override
  String get appTitle => 'Psalms';

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
  String get deviceNameGenericHint =>
      'iOS verrät der App nur das Modell. Gib diesem Gerät einen Namen, an dem du es erkennst, zum Beispiel \"Annas iPad\".';

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
  String get connectErrorSignedOut =>
      'Der Server kennt dieses Gerät nicht mehr. Koppele es mit einem neuen Code erneut; die alte Kopplung bleibt bis dahin erhalten.';

  @override
  String get tooltipTimeBook =>
      'Zeit der ganzen CD. Tippen für die Zeit dieses Titels.';

  @override
  String get tooltipTimeTrack =>
      'Zeit dieses Titels. Tippen für die Zeit der ganzen CD.';

  @override
  String get formatNotPlayableIos =>
      'Dieses Dateiformat lässt sich auf Apple-Geräten (iPhone, iPad, Mac) noch nicht abspielen.';

  @override
  String get connectSearching => 'Suche den Server erneut ...';

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
  String get qrButton => 'QR-Code scannen';

  @override
  String get qrScanTitle => 'Kopplungscode scannen';

  @override
  String get qrScanHint =>
      'Halte die Kamera auf den QR-Code der Kopplungsseite deines Servers.';

  @override
  String get qrScanNotPairing => 'Das ist kein Kopplungscode.';

  @override
  String get qrScanUsePicture => 'Stattdessen ein Bild wählen';

  @override
  String get qrScanTorch => 'Taschenlampe';

  @override
  String get qrScanPermissionDenied =>
      'Die Kamera ist für Psalms nicht erlaubt. Erlaube sie in den Einstellungen, oder wähle stattdessen ein Bild des QR-Codes.';

  @override
  String get qrScanOpenSettings => 'Einstellungen öffnen';

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
  String get settingsGroupGeneral => 'Allgemein';

  @override
  String get settingsGroupOffline => 'Offline und Downloads';

  @override
  String get settingsGroupLibrary => 'Bibliothek';

  @override
  String get settingsGroupDevices => 'Weitere Geräte';

  @override
  String get settingsGroupServer => 'Server';

  @override
  String sectionHeardBy(String name) {
    return 'Zuletzt gehört von $name';
  }

  @override
  String get sectionMyMediaTime => 'Meine Medienzeit';

  @override
  String sectionMediaTimeOf(String name) {
    return 'Medienzeit von $name';
  }

  @override
  String get chartTotal => 'Gesamt';

  @override
  String get chartNothing => 'In den letzten Tagen wurde nichts gehört.';

  @override
  String chartMinutes(int n) {
    return '$n Min.';
  }

  @override
  String chartHours(int n) {
    return '$n Std.';
  }

  @override
  String get settingsShareListening => 'Zeigen, was ich höre';

  @override
  String get settingsShareListeningHint =>
      'Die Familie sieht deine zuletzt gehörten Bücher auf ihrer Startseite.';

  @override
  String get settingsShareListeningLocked =>
      'Ein Administrator hat festgelegt, dass die Familie sieht, was du hörst.';

  @override
  String get settingsShareListeningFailed =>
      'Diese Einstellung ließ sich nicht ändern.';

  @override
  String get settingsTheme => 'Farben';

  @override
  String get themeSystem => 'System';

  @override
  String get themeLight => 'Hell';

  @override
  String get themeDark => 'Dunkel';

  @override
  String get settingsDisconnectHint =>
      'Vergisst die Kopplung mit dem Server auf diesem Gerät. Zum erneuten Verbinden brauchst du einen neuen Kopplungscode.';

  @override
  String get settingsPlayback => 'Wiedergabe';

  @override
  String get settingsBuffer => 'Audiopuffer';

  @override
  String settingsBufferSeconds(int seconds) {
    return '$seconds s';
  }

  @override
  String get settingsBufferHint =>
      'Wie viel Audio bereitgehalten wird, bevor es gespielt wird. Ein größerer Puffer überbrückt ein langsames Netz oder eine langsame Platte, ein kleinerer lässt einen Titelwechsel früher hören. Gilt ab dem nächsten Titel.';

  @override
  String get settingsDevices => 'Geräte';

  @override
  String settingsDevicesCount(int used, int max) {
    return '$used von $max Geräten angebunden';
  }

  @override
  String settingsDevicesNoLimit(int used) {
    return '$used Geräte angebunden';
  }

  @override
  String get settingsConnectDevice => 'Weiteres Gerät anbinden';

  @override
  String get settingsConnectDeviceHint =>
      'Zeigt einen QR-Code, den das andere Gerät scannt. Der Code gilt einmal und nur kurze Zeit.';

  @override
  String get pairQrScan =>
      'Scanne diesen Code mit Psalms auf dem anderen Gerät.';

  @override
  String pairQrExpires(String time) {
    return 'Gültig noch $time';
  }

  @override
  String get pairQrExpired => 'Der Code ist abgelaufen.';

  @override
  String get pairQrNew => 'Neuer Code';

  @override
  String get pairQrFull =>
      'Alle Geräte sind angebunden. Entferne in der Verwaltung des Servers eines, um ein weiteres anzubinden.';

  @override
  String get pairQrError =>
      'Der Code konnte nicht erstellt werden. Versuche es erneut.';

  @override
  String get pairQrClose => 'Schließen';

  @override
  String get pairQrCopy => 'Adresse kopieren';

  @override
  String get pairQrCopied =>
      'Adresse kopiert. Füge sie im Verbindungsbildschirm des anderen Geräts ein.';

  @override
  String get streamArtist => 'Interpret';

  @override
  String get streamTrack => 'Titel';

  @override
  String get streamStation => 'Sender';

  @override
  String get streamGenre => 'Genre';

  @override
  String get streamDescription => 'Über';

  @override
  String get streamQuality => 'Qualität';

  @override
  String get streamWebsite => 'Webseite';

  @override
  String get streamWaiting => 'Warte auf die Angaben des Senders …';

  @override
  String get cancel => 'Abbrechen';

  @override
  String aboutVersion(String version) {
    return 'Version $version';
  }

  @override
  String get aboutDescription =>
      'Psalms spielt Hörbücher, Musik und mehr von deinem eigenen David-Server, für die ganze Familie.';

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
      'Psalms ist nur der Player. Es braucht einen David-Server, auf dem deine Hörbücher, deine Musik und die Profile der Familie liegen. Ohne einen gibt es in Psalms nichts zu hören.';

  @override
  String get aboutDavidTitle => 'Psalms braucht einen David-Server';

  @override
  String get aboutDavidText =>
      'Psalms bringt selbst keine Inhalte mit. Es spielt, was ein David-Server anbietet: Hörbücher, Musik und mehr aus deiner eigenen Bibliothek, mit einem Profil für jedes Familienmitglied. David ist freie Software, die du selbst betreibst, auf einem Heimserver oder einem kleinen Rechner. Wer ihn betreibt, erzeugt auf der Verwaltungsseite einen Kopplungscode; den gibst du hier einmal ein. Ohne David-Server kannst du nur die Demodaten ausprobieren.';

  @override
  String get aboutDavidSteps =>
      '1. David-Server betreiben.  2. Seine Verwaltungsseite öffnen und „Gerät koppeln“ wählen.  3. Den Code in Psalms scannen oder einfügen.';

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
      'Psalms spricht nur mit dem David-Server, mit dem es gekoppelt ist, über eine Verbindung, die das Zertifikat des Servers prüft. Es gibt keine Analyse und keine Werbung. Die Kopplung und die heruntergeladenen Dateien bleiben auf diesem Gerät.';

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

  @override
  String greetingNamed(String greeting, String name) {
    return '$greeting, $name';
  }

  @override
  String get sectionRecent => 'Zuletzt gehört';

  @override
  String get genreOther => 'Sonstige';

  @override
  String get sectionAlbums => 'Alben';

  @override
  String get sectionBooks => 'Bücher';

  @override
  String get sectionNarrators => 'Sprecher';

  @override
  String get sectionAlbumsAndBooks => 'Alben und Bücher';

  @override
  String get navPodcasts => 'Podcasts';

  @override
  String get navRadio => 'Radio';

  @override
  String get podcastAdd => 'Podcast abonnieren';

  @override
  String get podcastAddress => 'Adresse des Feeds oder der Webseite';

  @override
  String get podcastFollow => 'Abonnieren';

  @override
  String podcastFollowed(String title) {
    return '„$title“ abonniert';
  }

  @override
  String get askToRelease =>
      'Bitte darum, dass es freigegeben wird. Es wartet, bis ein Elternteil es erlaubt.';

  @override
  String get podcastInvalid =>
      'Unter dieser Adresse gibt es keinen Podcast-Feed.';

  @override
  String get notAllowed => 'Das darfst du nicht.';

  @override
  String get serverUnreachableShort => 'Der Server ist nicht erreichbar.';

  @override
  String get needsServer =>
      'Podcasts und Radio brauchen eine Verbindung zu deinem Server.';

  @override
  String get noPodcastsYet => 'Noch keine Podcasts. Mit + einen abonnieren.';

  @override
  String get waitingForRelease =>
      'Wartet auf eine Freigabe. Bitte darum, dass es freigegeben wird.';

  @override
  String get feedProblem => 'Der Feed konnte zuletzt nicht gelesen werden.';

  @override
  String get podcastUnfollowTitle => 'Diesen Podcast nicht mehr abonnieren?';

  @override
  String podcastUnfollowAsk(String title) {
    return '„$title“ verschwindet aus den Podcasts dieses Kontos, auf allen seinen Geräten. Mit der Adresse des Feeds kannst du ihn wieder abonnieren.';
  }

  @override
  String get stationRemoveTitle => 'Diesen Sender entfernen?';

  @override
  String stationRemoveAsk(String name) {
    return '„$name“ wird aus den Favoriten dieses Kontos entfernt.';
  }

  @override
  String get podcastUnfollow => 'Nicht mehr abonnieren';

  @override
  String get noEpisodesYet => 'Noch keine Folgen.';

  @override
  String episodeAt(String time) {
    return 'bei $time angehalten';
  }

  @override
  String get stationAdd => 'Sender hinzufügen';

  @override
  String get stationName => 'Name';

  @override
  String get stationAddress => 'Adresse des Streams';

  @override
  String get stationSave => 'Speichern';

  @override
  String stationAdded(String name) {
    return '„$name“ hinzugefügt';
  }

  @override
  String get stationInvalid =>
      'Ein Sender braucht einen Namen und die Adresse seines Streams (http oder https).';

  @override
  String get noStationsYet => 'Noch keine Sender. Mit + einen hinzufügen.';

  @override
  String get stationRemove => 'Aus den Favoriten entfernen';

  @override
  String get radioLive => 'Live';

  @override
  String get cdImportButton => 'CD importieren';

  @override
  String get cdImportTooltip =>
      'Die CD im Laufwerk in die Bibliothek importieren';

  @override
  String get cdIdentifying => 'Die CD wird gelesen…';

  @override
  String get cdFound =>
      'Die CD wurde online gefunden. Bitte die Angaben prüfen.';

  @override
  String get cdNotFound =>
      'Die CD wurde online nicht gefunden. Bitte die Angaben eingeben.';

  @override
  String get cdCandidate => 'Treffer';

  @override
  String get cdSearchLabel =>
      'Nicht der richtige Treffer? MusicBrainz suchen (Titel, Autor)';

  @override
  String get cdSearchGo => 'Suchen';

  @override
  String get cdSearchNothing => 'Nichts gefunden.';

  @override
  String get cdAuthor => 'Autor';

  @override
  String get cdTitle => 'Titel';

  @override
  String get cdDiscNumber => 'CD-Nummer';

  @override
  String get cdDiscsTotal => 'Anzahl CDs';

  @override
  String get cdKind => 'Art';

  @override
  String get cdKindAudiobook => 'Hörbuch';

  @override
  String get cdKindMusic => 'Musik';

  @override
  String get cdEjectAfter => 'CD danach auswerfen';

  @override
  String cdTracks(int count) {
    return '$count Titel';
  }

  @override
  String get cdSave => 'Importieren';

  @override
  String get cdClose => 'Schließen';

  @override
  String get cdStopImport => 'Import abbrechen';

  @override
  String cdReading(int track, int total) {
    return 'Titel $track von $total wird gelesen';
  }

  @override
  String get cdDone => 'Die CD ist jetzt in der Bibliothek.';

  @override
  String get cdEjected => 'Die CD wurde ausgeworfen.';

  @override
  String get cdCancelled =>
      'Der Import wurde abgebrochen. Es wurde nichts in die Bibliothek aufgenommen.';

  @override
  String get cdErrNoDisc => 'Im Laufwerk liegt keine lesbare Audio-CD.';

  @override
  String get cdErrUnsupported =>
      'Der Server verlangt ein Format oder eine Tag-Version, die diese App noch nicht erzeugen kann.';

  @override
  String get cdErrRead =>
      'Die CD konnte nicht zuverlässig gelesen werden. Sie ist vielleicht zerkratzt oder schmutzig.';

  @override
  String get cdErrDriveGone => 'Das Laufwerk des Servers antwortet nicht mehr.';

  @override
  String get cdErrEncode => 'Der Server konnte die Audiodaten nicht umwandeln.';

  @override
  String get cdErrForbidden =>
      'Dein Konto darf keine CDs in die Bibliothek aufnehmen.';

  @override
  String get cdErrDetails => 'Einzelheiten';

  @override
  String get cdErrVerify =>
      'Zwei Lesedurchgänge eines Titels ergaben nie dasselbe Ergebnis. Die CD ist vielleicht zerkratzt oder schmutzig.';

  @override
  String get cdErrCancelled =>
      'Der Import wurde abgebrochen. Es wurde nichts in die Bibliothek aufgenommen.';

  @override
  String get cdErrIo =>
      'Auf dem Server konnte eine Datei nicht geschrieben werden.';

  @override
  String get cdErrNetwork => 'Der Server antwortet nicht.';

  @override
  String get cdErrCertificate =>
      'Das Zertifikat des Servers ist nicht das, mit dem dieses Gerät gekoppelt wurde.';

  @override
  String get cdErrUnauthorized =>
      'Dieses Gerät ist am Server nicht mehr angemeldet.';

  @override
  String get cdErrInvalidToc =>
      'Der Server konnte das Inhaltsverzeichnis dieser CD nicht verwenden.';

  @override
  String get cdErrInvalidQuery =>
      'Die Suche braucht mindestens zwei Buchstaben.';

  @override
  String get cdErrNoContact =>
      'Der Administrator hat noch keine Kontaktadresse für MusicBrainz eingetragen, deshalb können CDs nicht nachgeschlagen werden. Die Angaben können von Hand eingegeben werden.';

  @override
  String get cdErrLookupFailed =>
      'Das Nachschlagen der CD im Internet hat nicht funktioniert. Die Angaben können von Hand eingegeben werden.';

  @override
  String get cdErrNotFound => 'Der Server kennt diesen Eintrag nicht.';

  @override
  String get cdErrInvalidRelease =>
      'Der Server hat die Veröffentlichung nicht akzeptiert.';

  @override
  String get cdErrTrackNumbers =>
      'Die Titelnummern sind nicht vollständig oder kommen doppelt vor.';

  @override
  String get cdErrInvalidMeta =>
      'Der Server hat Autor, Titel oder Nummern nicht akzeptiert.';

  @override
  String get cdErrUnreadableFile =>
      'Der Server konnte eine der Dateien nicht lesen.';

  @override
  String get cdErrFormatMismatch =>
      'Eine Datei hat nicht das Format, das die Einstellungen des Servers verlangen.';

  @override
  String get cdErrBadExtension =>
      'Eine Datei hat eine Endung, die der Server nicht akzeptiert.';

  @override
  String get cdErrUnknownFile =>
      'Der Server hat eine Datei erhalten, die er nicht erwartet hat.';

  @override
  String get cdErrMissingFile =>
      'In dem, was an den Server gesendet wurde, fehlt eine Datei.';

  @override
  String get cdErrBadCover => 'Das Titelbild wurde nicht akzeptiert.';

  @override
  String get cdErrBadTags =>
      'Der Server konnte die Tags einer Datei nicht schreiben.';

  @override
  String get cdErrNoImportFolder =>
      'Der Server hat für diese Medienart keinen Importordner.';

  @override
  String get cdErrNoSpace => 'Auf dem Server ist nicht genug Platz.';

  @override
  String get cdErrDiscExists =>
      'Diese CD ist schon in der Bibliothek; der Server überschreibt sie nicht.';

  @override
  String get cdErrTooLarge => 'Der Upload ist größer, als der Server annimmt.';

  @override
  String get cdErrBadRequest => 'Der Server hat die Anfrage nicht verstanden.';

  @override
  String get cdErrInvalidJob =>
      'Der Server hat den Importauftrag nicht akzeptiert.';

  @override
  String get cdErrJobClosed => 'Der Importauftrag ist schon beendet.';

  @override
  String get cdErrImportFailed =>
      'Der Server konnte die CD nicht in die Bibliothek aufnehmen.';

  @override
  String get cdErrInterrupted =>
      'Der Import wurde unterbrochen, zum Beispiel durch einen Neustart des Servers.';

  @override
  String get cdErrStale =>
      'Der Import hat keine Nachricht mehr gesendet und wurde aufgegeben.';

  @override
  String get cdErrClient =>
      'Das Gerät, das den Import gestartet hat, hat ihn aufgegeben.';

  @override
  String cdErrUnknown(String code) {
    return 'Der CD-Import ist fehlgeschlagen ($code).';
  }

  @override
  String get sectionNew => 'Neu';

  @override
  String get badgeNew => 'Neu';

  @override
  String get tooltipPlayed => 'Gespielt';

  @override
  String playsCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count-mal vollständig gehört',
      one: 'Einmal vollständig gehört',
    );
    return '$_temp0';
  }

  @override
  String lastPlayedOn(DateTime date) {
    final intl.DateFormat dateDateFormat = intl.DateFormat.yMMMd(localeName);
    final String dateString = dateDateFormat.format(date);

    return 'zuletzt gespielt am $dateString';
  }

  @override
  String get cdStarting => 'Wird gestartet…';

  @override
  String get cdEncoding => 'Die Audiodaten werden umgewandelt…';

  @override
  String get cdAddingToLibrary => 'Die CD wird in die Bibliothek aufgenommen…';

  @override
  String get cdImportingTooltip => 'CD wird importiert';

  @override
  String get cdEjectTooltip => 'CD auswerfen';

  @override
  String get cdNothingToEject => 'Es gibt nichts auszuwerfen.';

  @override
  String get cdErrNoDrive => 'Der Server hat kein CD-Laufwerk.';

  @override
  String get cdErrDriveDenied =>
      'Der Server darf sein CD-Laufwerk nicht benutzen. Der Administrator muss es erlauben.';

  @override
  String get cdErrTrayOpen =>
      'Das Fach des Laufwerks ist offen. Bitte zuerst von Hand schließen.';

  @override
  String get cdErrBusy => 'Das Laufwerk ist mit einer anderen CD beschäftigt.';

  @override
  String get cdErrEjectFailed => 'Das Laufwerk hat sein Fach nicht geöffnet.';

  @override
  String get cdErrInternal => 'Der Server hatte ein internes Problem.';

  @override
  String get cdErrInvalidSelection =>
      'Ein gewählter Titel ist nicht auf der CD oder doppelt gewählt.';

  @override
  String get cdSelectAll => 'Alle Titel auswählen';

  @override
  String get cdSelectNone => 'Keinen Titel auswählen';

  @override
  String get cdSelectInvert => 'Auswahl umkehren';

  @override
  String cdTrackDefault(String number) {
    return 'Titel $number';
  }

  @override
  String cdTracksSummary(int selected, int total, String duration) {
    return '$selected von $total Titeln, $duration';
  }

  @override
  String get cdErrEmptySelection => 'Es ist kein Titel gewählt.';

  @override
  String cdErrTaken(String numbers) {
    return 'Titel $numbers sind schon in der Bibliothek.';
  }

  @override
  String cdBarHead(String title, int disc) {
    return '$title, CD $disc';
  }

  @override
  String cdBarTrack(int track, int total) {
    return 'Titel $track von $total';
  }

  @override
  String get cdPhaseReading => 'Lesen';

  @override
  String get cdPhaseChecking => 'Prüfen';

  @override
  String get cdPhaseEncoding => 'Kodieren';

  @override
  String get cdPhasePlacing => 'Einsortieren';

  @override
  String get cdPhaseImporting => 'Importieren';

  @override
  String get cdPhaseWorking => 'In Arbeit';

  @override
  String cdBarDone(int count) {
    return 'Fertig: $count Titel';
  }

  @override
  String get cdBarDoneNoCount => 'Fertig';

  @override
  String cdBarImporting(String line) {
    return 'Import von: $line';
  }

  @override
  String get cdErrDiscPresent => 'Diese CD ist schon in der Bibliothek.';

  @override
  String get cdErrNotPresent =>
      'Diese CD ist nicht in der Bibliothek, also gibt es nichts zu reparieren.';

  @override
  String get cdErrNothingToHeal =>
      'Die CD ist vollständig in der Bibliothek; es gibt nichts zu reparieren.';

  @override
  String get cdErrUnreadableTrack =>
      'Einzelne Titel konnten von der CD nicht gelesen werden (sie ist vielleicht zerkratzt).';

  @override
  String get cdStateUnverified => 'Übernommen, nicht bestätigt';

  @override
  String get cdErrUnverifiedTrack =>
      'Ein Titel wurde nicht zweimal gleich gelesen und mit der ersten Lesung übernommen (nicht bestätigt).';

  @override
  String get cdErrResumeMismatch =>
      'Die Einstellungen oder die Titelauswahl haben sich geändert, deshalb wird die CD von vorn gelesen.';

  @override
  String get cdErrHealNeeded =>
      'Die CD ist in der Bibliothek, aber es fehlen Titel oder sie sind defekt. Im Import-Dialog lässt sie sich reparieren.';

  @override
  String get cdErrResumePending =>
      'Ein unterbrochenes Lesen dieser CD wartet. Im Import-Dialog lässt es sich fortsetzen.';

  @override
  String get cdErrAmbiguousMatch =>
      'Mehrere Veröffentlichungen passen zu dieser CD. Bitte eine auswählen.';

  @override
  String get cdErrNoMatch => 'Die Online-Datenbank kennt diese CD nicht.';

  @override
  String get cdErrNoTarget =>
      'Der Server hat keinen Importordner, in den er schreiben darf.';

  @override
  String get cdErrUdevOff =>
      'Der automatische Import braucht die Erkennung der Laufwerke über Systemereignisse (eingeschaltet).';

  @override
  String get cdErrNotAudio => 'Das ist keine Audio-CD.';

  @override
  String get cdErrStartFailed => 'Der Import konnte nicht gestartet werden.';

  @override
  String get cdErrLocked =>
      'Die CD wurde hinzugefügt, aber der Import der Bibliothek ist gesperrt. Sie erscheint, wenn die Sperre aufgehoben ist.';

  @override
  String get cdContinue => 'Fortsetzen';

  @override
  String cdResumeNote(int done, int total, String when) {
    return 'Diese CD wurde schon bis Titel $done von $total gelesen (unterbrochen $when).';
  }

  @override
  String get cdRereadFromStart => 'Von vorn noch einmal lesen';

  @override
  String get cdResumeLost =>
      'Ein früheres Lesen dieser CD lässt sich nicht fortsetzen, weil sich die Einstellungen geändert haben. Die CD wird neu gelesen.';

  @override
  String get cdResumed => 'Ein früheres Lesen wird fortgesetzt.';

  @override
  String get cdImportAnyway => 'Trotzdem importieren';

  @override
  String get cdHeal => 'Prüfen und reparieren';

  @override
  String get cdForceTitle => 'CD erneut importieren?';

  @override
  String get cdForceBody =>
      'Diese CD ist schon in der Bibliothek. So kommt sie ein zweites Mal als neue Kopie hinzu. Das nur tun, wenn wirklich zwei Kopien gewollt sind.';

  @override
  String get cdForceConfirm => 'Als neue Kopie importieren';

  @override
  String get cdPresentTitle => 'Diese CD ist schon in der Bibliothek';

  @override
  String cdPresentBookDisc(String book, int disc) {
    return '$book, CD $disc';
  }

  @override
  String cdPresentBookAuthorDisc(String book, String author, int disc) {
    return '$book von $author, CD $disc';
  }

  @override
  String cdPresentComplete(int inLibrary, int onDisc) {
    return 'Die CD ist vollständig in der Bibliothek ($inLibrary von $onDisc Titeln).';
  }

  @override
  String cdPresentCount(int inLibrary, int onDisc) {
    return '$inLibrary von $onDisc Titeln sind in der Bibliothek.';
  }

  @override
  String get cdMissing => 'Fehlt';

  @override
  String get cdDamaged => 'Defekt';

  @override
  String get cdDamagedEmpty => 'die Datei ist leer';

  @override
  String get cdDamagedUnreadable => 'die Datei lässt sich nicht lesen';

  @override
  String cdDamagedLength(String have, String want) {
    return 'die Datei ist $have lang, die CD hat $want';
  }

  @override
  String get cdDamagedOther => 'die Datei passt nicht zur CD';

  @override
  String get cdHealHint =>
      'Von der CD werden nur die fehlenden und defekten Titel gelesen. Eine defekte Datei bleibt im Quarantäne-Ordner erhalten.';

  @override
  String get cdStateWaiting => 'Wartet';

  @override
  String get cdStateRead => 'Gelesen, wartet auf die Prüfung';

  @override
  String cdSpeed(String x) {
    return 'Laufwerk liest mit ${x}x';
  }

  @override
  String get cdStateRereading => 'Liest erneut';

  @override
  String cdStateRereadingPercent(int percent) {
    return 'Liest erneut $percent %';
  }

  @override
  String get cdStateReading => 'Wird gelesen';

  @override
  String cdStateReadingPercent(int percent) {
    return 'Wird gelesen $percent %';
  }

  @override
  String get cdStateVerifying => 'Wird geprüft';

  @override
  String get cdStateEncoding => 'Wird kodiert';

  @override
  String get cdStateDone => 'Fertig';

  @override
  String get cdStateFailed => 'Fehler';

  @override
  String cdBuffer(int seconds, int max) {
    return 'Puffer $seconds s von $max s';
  }

  @override
  String get filterPlaylists => 'Playlists';

  @override
  String get playlistNew => 'Neue Playlist';

  @override
  String get playlistNewItem => 'Neue Playlist …';

  @override
  String get playlistName => 'Name';

  @override
  String get playlistSave => 'Speichern';

  @override
  String get playlistsEmpty => 'Noch keine Playlists. Mit + eine anlegen.';

  @override
  String playlistMeta(int count, String duration) {
    return '$count Titel · $duration';
  }

  @override
  String get playlistAddTo => 'Zur Playlist hinzufügen';

  @override
  String playlistAddedTo(String name) {
    return 'Zu „$name“ hinzugefügt.';
  }

  @override
  String get playlistChoose => 'Zu welcher Playlist hinzufügen?';

  @override
  String get playlistPlay => 'Abspielen';

  @override
  String get playlistRename => 'Umbenennen';

  @override
  String get playlistRenameTitle => 'Playlist umbenennen';

  @override
  String get playlistDelete => 'Playlist löschen';

  @override
  String get playlistDeleteTitle => 'Diese Playlist löschen?';

  @override
  String playlistDeleteText(String name) {
    return '„$name“ wird aus deinem Profil gelöscht. Die Titel bleiben in der Bibliothek.';
  }

  @override
  String get playlistRemoveEntry => 'Aus der Playlist entfernen';

  @override
  String get playlistUnavailable => 'Nicht verfügbar';

  @override
  String get playlistEmpty =>
      'Diese Playlist ist leer. „Zur Playlist hinzufügen“ gibt es bei einem Titel und bei einem Buch.';

  @override
  String get playlistDownload => 'Playlist herunterladen';

  @override
  String get plErrNameTaken => 'Du hast schon eine Playlist mit diesem Namen.';

  @override
  String get plErrInvalidName =>
      'Der Name darf nicht leer sein und höchstens 100 Zeichen haben.';

  @override
  String get plErrUnknownPart =>
      'Ein Titel ist nicht mehr da, oder du darfst ihn nicht verwenden.';

  @override
  String get plErrUnknownBook =>
      'Das Buch ist nicht mehr da, oder du darfst es nicht verwenden.';

  @override
  String get plErrInvalidOrder =>
      'Die Reihenfolge war nicht verwendbar. Die Liste steht so, wie sie auf dem Server ist.';

  @override
  String get plErrInvalidPosition =>
      'Diesen Platz in der Playlist gibt es nicht.';

  @override
  String get plErrInvalidRequest => 'Die Anfrage war nicht gültig.';

  @override
  String get plErrPlaylistFull =>
      'Die Playlist ist voll (höchstens 5000 Titel).';

  @override
  String get plErrTooManyPlaylists =>
      'Du hast die höchste Zahl an Playlists (100).';

  @override
  String get plErrNotFound => 'Die Playlist gibt es nicht mehr.';

  @override
  String get plErrNetwork => 'Der Server antwortet nicht.';

  @override
  String plErrUnknown(String code) {
    return 'Die Playlist konnte nicht geändert werden ($code).';
  }

  @override
  String get hideMenuTooltip => 'Aus- oder einblenden';

  @override
  String get hideBook => 'Dieses Buch ausblenden';

  @override
  String get showBook => 'Dieses Buch wieder einblenden';

  @override
  String hideAuthorAudiobooks(String name) {
    return 'Alle Hörbücher von $name ausblenden';
  }

  @override
  String showAuthorAudiobooks(String name) {
    return 'Die Hörbücher von $name wieder einblenden';
  }

  @override
  String hideAuthorMusic(String name) {
    return 'Alle Musik von $name ausblenden';
  }

  @override
  String showAuthorMusic(String name) {
    return 'Die Musik von $name wieder einblenden';
  }

  @override
  String hideAuthorAll(String name) {
    return 'Alles von $name ausblenden';
  }

  @override
  String showAuthorAll(String name) {
    return 'Alles von $name wieder einblenden';
  }

  @override
  String get hiddenMark => 'Ausgeblendet';

  @override
  String get hidingFailed => 'Die Änderung konnte nicht gespeichert werden.';

  @override
  String get settingsMyLibrary => 'Meine Bibliothek';

  @override
  String get settingsShowHidden => 'Ausgeblendetes anzeigen';

  @override
  String get settingsShowHiddenHint =>
      'Was du ausblendest, verschwindet aus deinen Listen, nur aus deinen. Mit diesem Schalter wird es wieder aufgeführt, abgedunkelt, damit du es wieder einblenden kannst.';

  @override
  String get settingsHiddenTitle => 'Ausgeblendet';

  @override
  String get settingsHiddenNone =>
      'Du hast nichts ausgeblendet. Halte ein Buch gedrückt oder öffne es und nimm das Menü oben.';

  @override
  String settingsHiddenAuthorAudiobooks(String name) {
    return 'Hörbücher von $name';
  }

  @override
  String settingsHiddenAuthorMusic(String name) {
    return 'Musik von $name';
  }

  @override
  String settingsHiddenAuthorAll(String name) {
    return 'Alles von $name';
  }

  @override
  String get settingsShowAgain => 'Wieder einblenden';

  @override
  String musicOf(String name) {
    return 'Musik von $name';
  }

  @override
  String get musicOfNobody => 'Weitere Musik';

  @override
  String ownerLabel(String name) {
    return 'Besitzer: $name';
  }

  @override
  String audiobooksOf(String name) {
    return 'Hörbücher von $name';
  }

  @override
  String get audiobooksOfNobody => 'Weitere Hörbücher';

  @override
  String get groupExpand => 'Aufklappen';

  @override
  String get groupCollapse => 'Zuklappen';

  @override
  String get settingsWebsite => 'Website des Servers';

  @override
  String get settingsWebsiteHint =>
      'Öffnet die Startseite des Servers im Browser, mit dem Weg zur Verwaltung.';

  @override
  String get settingsWebsiteFailed =>
      'Der Browser konnte nicht geöffnet werden.';

  @override
  String get shareTooltip => 'Teilen';

  @override
  String shareText(String title, String author) {
    return '$title von $author';
  }

  @override
  String get labelsTitle => 'Schlagwörter';

  @override
  String get labelsEdit => 'Schlagwörter bearbeiten';

  @override
  String get labelsNone => 'Noch keine Schlagwörter.';

  @override
  String get trackLabelsEdit => 'Schlagwörter dieses Titels';

  @override
  String get labelsAdd => 'Hinzufügen';

  @override
  String get labelsNew => 'Neues Schlagwort';

  @override
  String get labelsSave => 'Speichern';

  @override
  String get labelsFailed =>
      'Die Schlagwörter konnten nicht gespeichert werden.';

  @override
  String get labelsBad =>
      'Das ist kein brauchbares Schlagwort: 1 bis 40 Zeichen, kein Komma, Semikolon oder senkrechter Strich.';

  @override
  String get labelsFilterClear => 'Alle Schlagwörter';

  @override
  String get labelsRename => 'Schlagwort umbenennen';

  @override
  String get labelsDelete => 'Schlagwort löschen';

  @override
  String get labelsNoneInLibrary =>
      'In der Bibliothek gibt es noch keine Schlagwörter. Der Besitzer eines Buchs setzt sie auf dessen Seite.';

  @override
  String labelsDeleteConfirm(String name, int count) {
    return '\"$name\" bei $count Büchern entfernen?';
  }

  @override
  String get offlineBannerManual =>
      'Offline-Modus · nur heruntergeladene Titel werden angezeigt';

  @override
  String get offlineBannerUnreachable =>
      'Server nicht erreichbar · nur heruntergeladene Titel werden angezeigt';

  @override
  String offlinePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count Änderungen warten auf den Server',
      one: '1 Änderung wartet auf den Server',
    );
    return '$_temp0';
  }

  @override
  String get offlineRetry => 'Erneut versuchen';

  @override
  String get offlineNeedsNetwork =>
      'Diese Seite braucht den Server. Sie ist wieder da, sobald du online bist.';

  @override
  String get offlineSettingTitle => 'Offline-Modus';

  @override
  String get offlineSettingHint =>
      'Zeigt nur die auf diesem Gerät heruntergeladenen Titel und fragt den Server nicht. Was du in der Zwischenzeit änderst (Schlagwörter, versteckte Alben, wo du aufgehört hast zu hören), wartet und wird beim Ausschalten gesendet.';

  @override
  String get offlineSyncNow => 'Änderungen jetzt senden';

  @override
  String get offlineSyncing => 'Änderungen werden gesendet …';

  @override
  String get offlineNothingWaits => 'Es warten keine Änderungen.';

  @override
  String get offlineRejectedTitle => 'Vom Server nicht übernommen';

  @override
  String offlineRejectedLabels(String title, String reason) {
    return 'Schlagwörter von „$title“ ($reason)';
  }

  @override
  String offlineRejectedHide(String title, String reason) {
    return 'Verstecken von „$title“ ($reason)';
  }

  @override
  String get offlineReasonForbidden => 'nicht erlaubt';

  @override
  String get offlineReasonGone => 'nicht mehr vorhanden';

  @override
  String get offlineReasonOther => 'abgelehnt';

  @override
  String get offlineDismiss => 'OK';

  @override
  String get profileTitle => 'Mein Profil';

  @override
  String get profileSettingsHint => 'Name, Bild, Sprache und PIN';

  @override
  String get profilePicture => 'Bild';

  @override
  String get profileTakePhoto => 'Foto aufnehmen';

  @override
  String get profileChoosePhoto => 'Bild auswählen';

  @override
  String get profileRemovePhotoTitle => 'Das Bild entfernen?';

  @override
  String get profileRemovePhotoAsk =>
      'Dein Bild wird auf dem Server und auf allen deinen Geräten gelöscht.';

  @override
  String get profileRemovePhoto => 'Bild entfernen';

  @override
  String get profileNickname => 'Spitzname';

  @override
  String get profileFirstName => 'Vorname';

  @override
  String get profileLastName => 'Nachname';

  @override
  String get profileLanguage => 'Sprache';

  @override
  String get profileLanguageAuto => 'Wie das Gerät';

  @override
  String get profileRole => 'Rolle';

  @override
  String get roleAdmin => 'Administrator';

  @override
  String get roleMember => 'Mitglied';

  @override
  String get roleChild => 'Kind';

  @override
  String get roleGuest => 'Gast';

  @override
  String get profileDevices => 'Meine Geräte';

  @override
  String get profileDeviceThis => 'dieses Gerät';

  @override
  String get profileNeverSeen => 'nie gesehen';

  @override
  String get profileSave => 'Speichern';

  @override
  String get profileSaved => 'Gespeichert.';

  @override
  String get profilePinTitle => 'PIN oder Passwort ändern';

  @override
  String get profilePinCurrent => 'Aktuelle PIN oder Passwort';

  @override
  String get profilePinNew => 'Neue PIN oder neues Passwort';

  @override
  String get profilePinRepeat => 'Neue wiederholen';

  @override
  String get profilePinChanged => 'Die PIN wurde geändert.';

  @override
  String get profileErrOffline =>
      'Dafür braucht es den Server. Versuche es erneut, wenn du verbunden bist.';

  @override
  String get profileErrExists => 'Dieser Spitzname ist vergeben.';

  @override
  String get profileErrName => 'Dieser Name geht nicht.';

  @override
  String get profileErrPinWrong => 'Die aktuelle PIN stimmt nicht.';

  @override
  String get profileErrPinInvalid =>
      'Die neue PIN oder das neue Passwort geht nicht.';

  @override
  String get profileErrPinMismatch =>
      'Die beiden neuen Eingaben unterscheiden sich.';

  @override
  String get profileErrLocked =>
      'Zu viele falsche Versuche. Versuche es später erneut.';

  @override
  String get profileErrAvatarType => 'Diese Datei ist kein Bild.';

  @override
  String get profileErrAvatarSize => 'Das Bild ist zu groß.';

  @override
  String get profileErrCameraDenied =>
      'Die Kamera oder die Fotos dürfen nicht benutzt werden. Erlaube es in den Einstellungen des Systems.';

  @override
  String get profileErrCameraNone => 'Hier gibt es keine Kamera.';

  @override
  String get profileErrGeneric => 'Das hat nicht geklappt.';

  @override
  String get profileAvatarTooltip => 'Dein Profil öffnen';

  @override
  String profileLastSeen(String date) {
    return 'zuletzt gesehen $date';
  }

  @override
  String profileDeviceCount(int used, int max) {
    return 'Geräte: $used von $max';
  }

  @override
  String profileDeviceCountFree(int used) {
    return 'Geräte: $used';
  }

  @override
  String get episodeDownload => 'Folge herunterladen';

  @override
  String get episodeDownloadCancel => 'Download abbrechen';

  @override
  String get episodeDownloaded => 'Auf diesem Gerät';

  @override
  String get episodeRemoveTitle => 'Download entfernen?';

  @override
  String get episodeRemoveText =>
      'Die Folge wird von diesem Gerät gelöscht. Sie bleibt im Podcast und lässt sich erneut herunterladen.';

  @override
  String episodeProgress(String done, String total) {
    return '$done von $total';
  }

  @override
  String get episodeErrorScheme =>
      'Diese Folge hat keine http- oder https-Adresse und lässt sich nicht herunterladen.';

  @override
  String get episodeErrorHttp => 'Der Anbieter hat den Download verweigert.';

  @override
  String episodeErrorTooLarge(String limit) {
    return 'Die Folge ist größer als die Grenze von $limit in den Einstellungen.';
  }

  @override
  String get episodeErrorNetwork =>
      'Der Download ist fehlgeschlagen: der Anbieter ist nicht erreichbar.';

  @override
  String get episodeErrorIo =>
      'Die Datei konnte nicht auf diesem Gerät gespeichert werden.';

  @override
  String podcastDownloadNewest(int count) {
    return 'Die neuesten $count ungehörten Folgen herunterladen';
  }

  @override
  String get podcastDownloadNewestNone =>
      'Es gibt nichts Neues zum Herunterladen.';

  @override
  String get podcastDownloadsTitle => 'Podcast-Downloads';

  @override
  String podcastDownloadsUsed(String size, int count) {
    return '$size in $count Folgen';
  }

  @override
  String get podcastDownloadsNone => 'Keine Podcast-Folgen auf diesem Gerät.';

  @override
  String get podcastDownloadsDeleteAll => 'Alle löschen';

  @override
  String get podcastDownloadsDeleteAllTitle =>
      'Alle Podcast-Downloads löschen?';

  @override
  String get podcastDownloadsDeleteAllText =>
      'Alle heruntergeladenen Folgen werden von diesem Gerät gelöscht. Die Podcasts behalten sie.';

  @override
  String get podcastDeleteAfter => 'Folgen nach dem Hören löschen';

  @override
  String get podcastDeleteAfterHint =>
      'Eine gehörte Folge wird von diesem Gerät gelöscht, ebenso eine, die der Podcast nicht mehr listet. Aus: nichts wird gelöscht, wenn du es nicht selbst tust.';

  @override
  String get podcastMaxSize => 'Größter Download';

  @override
  String get podcastDownloadAll => 'Gesamte Sammlung laden';

  @override
  String get podcastDownloadAllTitle => 'Die gesamte Sammlung laden?';

  @override
  String podcastDownloadAllText(int count, String size) {
    return '$count Folgen, etwa $size. Sie werden nacheinander geladen und belegen so viel Platz auf diesem Gerät. Du kannst jederzeit abbrechen.';
  }

  @override
  String podcastDownloadAllLimited(
    int count,
    String size,
    String limit,
    int fitCount,
    String fitSize,
  ) {
    return 'Die Sammlung hat $count Folgen, etwa $size. Das ist mehr als deine Grenze von $limit, deshalb werden nur die neuesten $fitCount (etwa $fitSize) geladen. Die Grenze kannst du unter Einstellungen > Podcast-Downloads erhöhen.';
  }

  @override
  String podcastDownloadAllBlocked(String limit) {
    return 'Schon die neueste Folge ist größer als deine Grenze von $limit. Die Grenze kannst du unter Einstellungen > Podcast-Downloads erhöhen.';
  }

  @override
  String get podcastDownloadAllStart => 'Laden';

  @override
  String podcastDownloadAllRunning(int done, int total) {
    return 'Lade $done von $total ...';
  }

  @override
  String get podcastDownloadAllStop => 'Stopp';

  @override
  String podcastDownloadAllDone(int count) {
    return '$count Folgen geladen.';
  }

  @override
  String get podcastBulkLimit => 'Größter Sammel-Download';

  @override
  String get podcastBulkLimitHint =>
      'Das Meiste, was „Gesamte Sammlung laden“ auf einmal lädt.';

  @override
  String get podcastBulkWifiOnly => 'Sammel-Downloads nur im WLAN';

  @override
  String get podcastBulkWifiOnlyHint =>
      '„Gesamte Sammlung laden“ lädt nicht über Mobilfunk. Kabel oder WLAN sind in Ordnung.';

  @override
  String get podcastDownloadAllMobile =>
      'Du bist im Mobilfunk. Die gesamte Sammlung wird nur im WLAN geladen (das kannst du unter Einstellungen > Podcast-Downloads ändern).';

  @override
  String podcastDownloadAllSpace(
    String free,
    int count,
    String size,
    int fitCount,
    String fitSize,
  ) {
    return 'Auf diesem Gerät sind nur $free frei, und 1 GB soll frei bleiben. Von den $count Folgen (etwa $size) werden die neuesten $fitCount (etwa $fitSize) geladen.';
  }

  @override
  String podcastDownloadAllNoSpace(String free) {
    return 'Auf diesem Gerät ist nicht genug Platz: nur $free sind frei, und 1 GB soll frei bleiben. Schaffe Platz und versuche es noch einmal.';
  }

  @override
  String podcastMegabytes(int mb) {
    return '$mb MB';
  }

  @override
  String get offlinePodcastsNone =>
      'Auf diesem Gerät sind noch keine Folgen. Lade Folgen herunter, solange du online bist, um sie hier zu hören.';

  @override
  String get barcodeCheckTooltip => 'Barcode prüfen';

  @override
  String get barcodeTitle => 'Barcode prüfen';

  @override
  String get barcodeFieldLabel => 'Barcode';

  @override
  String get barcodeCheck => 'Prüfen';

  @override
  String get barcodeInLibrary => 'In der Bibliothek:';

  @override
  String barcodeNotInLibrary(String code) {
    return 'Nicht in der Bibliothek: $code';
  }

  @override
  String get barcodeInvalid =>
      'Das ist kein gültiger Barcode. Prüfe die Ziffern.';

  @override
  String get barcodeOfflineNotFound =>
      'Die Bibliothek auf diesem Gerät hat es nicht. Der Server wurde nicht gefragt, weil du offline bist.';

  @override
  String get barcodeLookupFailed =>
      'Der Server konnte nicht gefragt werden. Versuche es später noch einmal.';

  @override
  String get barcodeOpen => 'Öffnen';

  @override
  String get barcodeScanHint => 'Halte den Barcode der CD vor die Kamera';

  @override
  String get barcodeTypeHint => 'Oder tippe die Ziffern unter dem Barcode ein:';
}
