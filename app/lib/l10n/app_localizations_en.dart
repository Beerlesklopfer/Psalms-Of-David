// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Psalm';

  @override
  String get navHome => 'Home';

  @override
  String get navSearch => 'Search';

  @override
  String get navLibrary => 'Library';

  @override
  String get greetingMorning => 'Good morning';

  @override
  String get greetingDay => 'Good afternoon';

  @override
  String get greetingEvening => 'Good evening';

  @override
  String get sectionContinue => 'Continue listening';

  @override
  String get sectionYourBooks => 'Your audiobooks';

  @override
  String get sectionAuthors => 'Authors';

  @override
  String get sectionChapters => 'Chapters';

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
      other: '$tracks tracks',
      one: '1 track',
    );
    return '$_temp0 · $_temp1 · $duration';
  }

  @override
  String durationHoursMinutes(int hours, int minutes) {
    return '$hours h $minutes min';
  }

  @override
  String durationMinutes(int minutes) {
    return '$minutes min';
  }

  @override
  String percentListened(int percent) {
    return '$percent% listened';
  }

  @override
  String get actionListen => 'Listen';

  @override
  String get actionContinue => 'Continue';

  @override
  String get tooltipPlay => 'Play';

  @override
  String get tooltipPause => 'Pause';

  @override
  String get tooltipClose => 'Close';

  @override
  String get tooltipClear => 'Clear';

  @override
  String get tooltipPrevious => 'Previous track';

  @override
  String get tooltipNext => 'Next track';

  @override
  String get tooltipBack10 => 'Back 10 seconds';

  @override
  String get tooltipForward30 => 'Forward 30 seconds';

  @override
  String get tooltipDownloadUnavailable => 'Download (not available yet)';

  @override
  String get nothingSelected => 'Nothing selected';

  @override
  String get searchHint => 'Title or author';

  @override
  String get nothingFound => 'Nothing found';

  @override
  String get filterBooks => 'Audiobooks';

  @override
  String get filterMusic => 'Music';

  @override
  String get noMusicYet => 'No music in the library yet';

  @override
  String get connectTitle => 'Connect to your server';

  @override
  String get connectIntro =>
      'Open the administration page of your server, create a pairing code and paste the address below it here.';

  @override
  String get pairingAddressLabel => 'Pairing address';

  @override
  String get deviceNameLabel => 'Name of this device';

  @override
  String get connectButton => 'Connect';

  @override
  String get retryButton => 'Try again';

  @override
  String get useDemoButton => 'Try with demo data';

  @override
  String get connectErrorAddress => 'This is not a pairing address.';

  @override
  String get connectErrorCode =>
      'The code is invalid, used or expired. Create a new one.';

  @override
  String get connectErrorLimit => 'This user has reached the limit of devices.';

  @override
  String get connectErrorNetwork => 'The server cannot be reached.';

  @override
  String get connectErrorCertificate =>
      'The server shows a different certificate than the one in the code.';

  @override
  String get connectErrorOther => 'Connecting failed.';

  @override
  String openPassage(String text) {
    return 'Open passage “$text”';
  }

  @override
  String get passageNotFound =>
      'No recording has this passage for you right now.';

  @override
  String get loadingFile => 'Loading…';

  @override
  String get pasteButton => 'Paste';

  @override
  String get qrButton => 'Read QR code from a picture';

  @override
  String get qrNotFound =>
      'No QR code found in this picture. Try a sharper or larger one.';

  @override
  String get qrChooseTitle => 'Choose a picture of the QR code';

  @override
  String bookPlayed(String time) {
    return 'Book: $time played';
  }

  @override
  String bookTotal(String time) {
    return 'of $time';
  }

  @override
  String get navSettings => 'Settings';

  @override
  String get navAbout => 'About';

  @override
  String get settingsServer => 'Server';

  @override
  String get settingsDemo =>
      'This is the demo library. No server is connected.';

  @override
  String get settingsAddress => 'Address';

  @override
  String get settingsDevice => 'This device';

  @override
  String get settingsDisconnect => 'Disconnect';

  @override
  String get settingsDisconnectHint =>
      'Forgets the pairing with the server on this device. To connect again you need a new pairing code.';

  @override
  String get cancel => 'Cancel';

  @override
  String aboutVersion(String version) {
    return 'Version $version';
  }

  @override
  String get aboutDescription =>
      'Psalm plays audiobooks, music and more from your own David server, for the whole family.';

  @override
  String get aboutLicense =>
      'Free software under the GNU Affero General Public License, version 3 or later.';

  @override
  String get aboutLicenses => 'Open-source licences';

  @override
  String get tooltipSpeed => 'Playback speed';

  @override
  String get aboutLicenseTitle => 'License';

  @override
  String get aboutScripture => 'Scripture';

  @override
  String get aboutFamily => 'Family';

  @override
  String get connectNeedsDavid =>
      'Psalm is only the player. It needs a David server, which holds your audiobooks, music and the family\'s profiles. Without one, Psalm has nothing to play.';

  @override
  String get aboutDavidTitle => 'Psalm needs a David server';

  @override
  String get aboutDavidText =>
      'Psalm does not come with any content. It plays what a David server offers: audiobooks, music and more from your own library, with a profile for each family member. David is free software that you run yourself, on a home server or a small computer. Whoever runs it creates a pairing code on its admin page; you enter that code here once. Without a David server you can only try the demo data.';

  @override
  String get aboutDavidSteps =>
      '1. Run a David server.  2. Open its admin page and choose Pair a device.  3. Scan or paste the code in Psalm.';

  @override
  String get tooltipRefresh => 'Refresh the library';

  @override
  String get tooltipDownload => 'Download for listening without the server';

  @override
  String get tooltipDownloadCancel => 'Stop the download';

  @override
  String get tooltipDownloaded => 'On this device. Tap to remove.';

  @override
  String downloadedCount(int done, int total) {
    return '$done of $total';
  }

  @override
  String get downloadRemoveTitle => 'Remove the download?';

  @override
  String get downloadRemoveText =>
      'The files are deleted from this device. The book stays on the server and can be downloaded again.';

  @override
  String get downloadRemove => 'Remove';

  @override
  String get aboutStorage => 'Storage on this device';

  @override
  String get aboutStorageNone => 'No downloads';

  @override
  String aboutStorageUsed(String size, int count) {
    return '$size in $count downloaded parts';
  }

  @override
  String get aboutStorageRemoveTitle => 'Remove all downloads?';

  @override
  String get craTitle => 'Security information (EU Cyber Resilience Act)';

  @override
  String get craManufacturer => 'Manufacturer';

  @override
  String get craVersion => 'Version and licence';

  @override
  String get craContact => 'Report a security problem to';

  @override
  String get craSupport => 'Security updates';

  @override
  String get craSupportOpen =>
      'Given with new versions. The end of the support period has not been set yet.';

  @override
  String craSupportUntil(String date) {
    return 'Until $date';
  }

  @override
  String get craData => 'Data and network';

  @override
  String get craDataText =>
      'Psalm talks only to the David server you paired it with, over a connection that checks the server\'s certificate. It has no analytics and no advertising. The pairing and the downloaded files stay on this device.';

  @override
  String get craComponents => 'Software components';

  @override
  String get craComponentsText =>
      'The list of all components with their licences.';

  @override
  String get aboutSource => 'Source code';

  @override
  String get filterVideos => 'Videos';

  @override
  String get filterEbooks => 'E-Books';

  @override
  String get comingSoonTitle => 'Not available yet';

  @override
  String get comingSoonVideos =>
      'Videos are planned. David does not manage them yet, so there is nothing to show here.';

  @override
  String get comingSoonEbooks =>
      'E-books and PDF files are planned. David does not manage them yet, so there is nothing to show here.';

  @override
  String get noAudiobooksYet => 'No audiobooks in the library yet';

  @override
  String readBy(String name) {
    return 'Read by $name';
  }

  @override
  String get filterAll => 'All';

  @override
  String get sectionArtists => 'Artists';

  @override
  String get sectionAuthorsAndArtists => 'Authors and artists';

  @override
  String artistLabel(String name) {
    return 'Artist: $name';
  }

  @override
  String get sectionYourMusic => 'Your music';

  @override
  String get craEmail => 'Contact';

  @override
  String get craWebsite => 'Website';

  @override
  String greetingNamed(String greeting, String name) {
    return '$greeting, $name';
  }

  @override
  String get sectionRecent => 'Recently listened';
}
