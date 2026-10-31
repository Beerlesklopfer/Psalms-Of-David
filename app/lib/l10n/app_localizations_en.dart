// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Psalms';

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
  String get deviceNameGenericHint =>
      'iOS tells the app only the model. Give this device a name you will recognise, for example \"Anna\'s iPad\".';

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
  String get connectErrorSignedOut =>
      'The server no longer knows this device. Pair it again with a new code; the old pairing stays until then.';

  @override
  String get tooltipTimeBook =>
      'Time of the whole CD. Tap for the time of this track.';

  @override
  String get tooltipTimeTrack =>
      'Time of this track. Tap for the time of the whole CD.';

  @override
  String get formatNotPlayableIos =>
      'This file format cannot be played on Apple devices (iPhone, iPad, Mac) yet.';

  @override
  String get connectSearching => 'Looking for the server again ...';

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
  String get qrButton => 'Scan the QR code';

  @override
  String get qrScanTitle => 'Scan the pairing code';

  @override
  String get qrScanHint =>
      'Hold the camera on the QR code of the pairing page of your server.';

  @override
  String get qrScanNotPairing => 'This is not a pairing code.';

  @override
  String get qrScanUsePicture => 'Choose a picture instead';

  @override
  String get qrScanTorch => 'Torch';

  @override
  String get qrScanPermissionDenied =>
      'The camera is not allowed for Psalms. Allow it in the settings, or choose a picture of the QR code instead.';

  @override
  String get qrScanOpenSettings => 'Open settings';

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
  String get settingsGroupGeneral => 'General';

  @override
  String get settingsGroupOffline => 'Offline and downloads';

  @override
  String get settingsGroupLibrary => 'Library';

  @override
  String get settingsGroupDevices => 'Further devices';

  @override
  String get settingsGroupServer => 'Server';

  @override
  String sectionHeardBy(String name) {
    return 'Recently heard by $name';
  }

  @override
  String get sectionMyMediaTime => 'My media time';

  @override
  String sectionMediaTimeOf(String name) {
    return 'Media time of $name';
  }

  @override
  String get chartTotal => 'Total';

  @override
  String get chartNothing => 'Nothing listened to in the last days.';

  @override
  String chartMinutes(int n) {
    return '$n min';
  }

  @override
  String chartHours(int n) {
    return '$n h';
  }

  @override
  String get settingsShareListening => 'Show what I listen to';

  @override
  String get settingsShareListeningHint =>
      'The family sees your recently heard books on their start page.';

  @override
  String get settingsShareListeningLocked =>
      'An administrator decided that your listening is visible to the family.';

  @override
  String get settingsShareListeningFailed => 'Could not change this setting.';

  @override
  String get settingsTheme => 'Colours';

  @override
  String get themeSystem => 'System';

  @override
  String get themeLight => 'Light';

  @override
  String get themeDark => 'Dark';

  @override
  String get settingsDisconnectHint =>
      'Forgets the pairing with the server on this device. To connect again you need a new pairing code.';

  @override
  String get settingsPlayback => 'Playback';

  @override
  String get settingsBuffer => 'Audio buffer';

  @override
  String settingsBufferSeconds(int seconds) {
    return '$seconds s';
  }

  @override
  String get settingsBufferHint =>
      'How much audio is kept ready before it is played. A larger buffer bridges a slow network or disk, a smaller one lets a change of track be heard sooner. It applies from the next track.';

  @override
  String get settingsDevices => 'Devices';

  @override
  String settingsDevicesCount(int used, int max) {
    return '$used of $max devices connected';
  }

  @override
  String settingsDevicesNoLimit(int used) {
    return '$used devices connected';
  }

  @override
  String get settingsConnectDevice => 'Connect another device';

  @override
  String get settingsConnectDeviceHint =>
      'Shows a QR code that the other device scans. The code works once and only for a short time.';

  @override
  String get pairQrScan => 'Scan this code with Psalms on the other device.';

  @override
  String pairQrExpires(String time) {
    return 'Valid for $time';
  }

  @override
  String get pairQrExpired => 'The code has expired.';

  @override
  String get pairQrNew => 'New code';

  @override
  String get pairQrFull =>
      'All devices are connected. Remove one in the administration of the server to connect another.';

  @override
  String get pairQrError => 'The code could not be made. Try again.';

  @override
  String get pairQrClose => 'Close';

  @override
  String get pairQrCopy => 'Copy the address';

  @override
  String get pairQrCopied =>
      'Address copied. Paste it into the connect screen of the other device.';

  @override
  String get streamArtist => 'Artist';

  @override
  String get streamTrack => 'Track';

  @override
  String get streamStation => 'Station';

  @override
  String get streamGenre => 'Genre';

  @override
  String get streamDescription => 'About';

  @override
  String get streamQuality => 'Quality';

  @override
  String get streamWebsite => 'Website';

  @override
  String get streamWaiting => 'Waiting for the information of the station …';

  @override
  String get cancel => 'Cancel';

  @override
  String aboutVersion(String version) {
    return 'Version $version';
  }

  @override
  String get aboutDescription =>
      'Psalms plays audiobooks, music and more from your own David server, for the whole family.';

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
      'Psalms is only the player. It needs a David server, which holds your audiobooks, music and the family\'s profiles. Without one, Psalms has nothing to play.';

  @override
  String get aboutDavidTitle => 'Psalms needs a David server';

  @override
  String get aboutDavidText =>
      'Psalms does not come with any content. It plays what a David server offers: audiobooks, music and more from your own library, with a profile for each family member. David is free software that you run yourself, on a home server or a small computer. Whoever runs it creates a pairing code on its admin page; you enter that code here once. Without a David server you can only try the demo data.';

  @override
  String get aboutDavidSteps =>
      '1. Run a David server.  2. Open its admin page and choose Pair a device.  3. Scan or paste the code in Psalms.';

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
      'Psalms talks only to the David server you paired it with, over a connection that checks the server\'s certificate. It has no analytics and no advertising. The pairing and the downloaded files stay on this device.';

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

  @override
  String get genreOther => 'Other';

  @override
  String get sectionAlbums => 'Albums';

  @override
  String get sectionBooks => 'Books';

  @override
  String get sectionNarrators => 'Narrators';

  @override
  String get sectionAlbumsAndBooks => 'Albums and books';

  @override
  String get navPodcasts => 'Podcasts';

  @override
  String get navRadio => 'Radio';

  @override
  String get podcastAdd => 'Follow a podcast';

  @override
  String get podcastAddress => 'Address of the feed or the web page';

  @override
  String get podcastFollow => 'Follow';

  @override
  String podcastFollowed(String title) {
    return 'Now following “$title”';
  }

  @override
  String get askToRelease =>
      'Please ask to have it released. It waits until a parent allows it.';

  @override
  String get podcastInvalid => 'That address gives no podcast feed.';

  @override
  String get notAllowed => 'You are not allowed to do that.';

  @override
  String get serverUnreachableShort => 'The server cannot be reached.';

  @override
  String get needsServer =>
      'Podcasts and radio need a connection to your server.';

  @override
  String get noPodcastsYet => 'No podcasts yet. Tap + to follow one.';

  @override
  String get waitingForRelease =>
      'Waiting for a release. Please ask to have it released.';

  @override
  String get feedProblem => 'The feed could not be read the last time.';

  @override
  String get podcastUnfollowTitle => 'Stop following this podcast?';

  @override
  String podcastUnfollowAsk(String title) {
    return '\"$title\" disappears from the podcasts of this account, on all its devices. You can follow it again with the address of its feed.';
  }

  @override
  String get stationRemoveTitle => 'Remove this station?';

  @override
  String stationRemoveAsk(String name) {
    return '\"$name\" is taken out of the favourites of this account.';
  }

  @override
  String get podcastUnfollow => 'Stop following';

  @override
  String get noEpisodesYet => 'No episodes yet.';

  @override
  String episodeAt(String time) {
    return 'stopped at $time';
  }

  @override
  String get stationAdd => 'Add a station';

  @override
  String get stationName => 'Name';

  @override
  String get stationAddress => 'Address of the stream';

  @override
  String get stationSave => 'Save';

  @override
  String stationAdded(String name) {
    return '“$name” added';
  }

  @override
  String get stationInvalid =>
      'A station needs a name and the address of its stream (http or https).';

  @override
  String get noStationsYet => 'No stations yet. Tap + to add one.';

  @override
  String get stationRemove => 'Remove from the favourites';

  @override
  String get radioLive => 'Live';

  @override
  String get cdImportButton => 'Import CD';

  @override
  String get cdImportTooltip => 'Import the CD in the drive into the library';

  @override
  String get cdIdentifying => 'Reading the disc…';

  @override
  String get cdFound => 'This disc was found online. Check the details.';

  @override
  String get cdNotFound =>
      'This disc was not found online. Please enter the details.';

  @override
  String get cdCandidate => 'Match';

  @override
  String get cdSearchLabel =>
      'Not the right match? Search MusicBrainz (title, author)';

  @override
  String get cdSearchGo => 'Search';

  @override
  String get cdSearchNothing => 'Nothing found.';

  @override
  String get cdAuthor => 'Author';

  @override
  String get cdTitle => 'Title';

  @override
  String get cdDiscNumber => 'CD number';

  @override
  String get cdDiscsTotal => 'Number of CDs';

  @override
  String get cdKind => 'Type';

  @override
  String get cdKindAudiobook => 'Audiobook';

  @override
  String get cdKindMusic => 'Music';

  @override
  String get cdEjectAfter => 'Eject the CD afterwards';

  @override
  String cdTracks(int count) {
    return '$count tracks';
  }

  @override
  String get cdSave => 'Import';

  @override
  String get cdClose => 'Close';

  @override
  String get cdStopImport => 'Stop import';

  @override
  String cdReading(int track, int total) {
    return 'Reading track $track of $total';
  }

  @override
  String get cdDone => 'The CD is now in the library.';

  @override
  String get cdEjected => 'The CD was ejected.';

  @override
  String get cdCancelled =>
      'The import was cancelled. Nothing was added to the library.';

  @override
  String get cdErrNoDisc => 'There is no readable audio CD in the drive.';

  @override
  String get cdErrUnsupported =>
      'The server asks for a format or tag version that this app cannot create yet.';

  @override
  String get cdErrRead =>
      'The disc could not be read reliably. It may be scratched or dirty.';

  @override
  String get cdErrDriveGone => 'The drive of the server stopped answering.';

  @override
  String get cdErrEncode => 'The server could not convert the audio.';

  @override
  String get cdErrForbidden => 'Your account may not add CDs to the library.';

  @override
  String get cdErrDetails => 'Details';

  @override
  String get cdErrVerify =>
      'Two readings of a track never gave the same result. The disc may be scratched or dirty.';

  @override
  String get cdErrCancelled =>
      'The import was cancelled. Nothing was added to the library.';

  @override
  String get cdErrIo => 'A file could not be written on the server.';

  @override
  String get cdErrNetwork => 'The server does not answer.';

  @override
  String get cdErrCertificate =>
      'The server\'s certificate is not the one this device was paired with.';

  @override
  String get cdErrUnauthorized =>
      'This device is no longer signed in to the server.';

  @override
  String get cdErrInvalidToc =>
      'The server could not use the table of contents of this disc.';

  @override
  String get cdErrInvalidQuery => 'The search needs at least two letters.';

  @override
  String get cdErrNoContact =>
      'The administrator has not set a contact address for MusicBrainz yet, so discs cannot be looked up. You can enter the details by hand.';

  @override
  String get cdErrLookupFailed =>
      'The online lookup of the disc did not work. You can enter the details by hand.';

  @override
  String get cdErrNotFound => 'The server does not know this item.';

  @override
  String get cdErrInvalidRelease => 'The server did not accept the release.';

  @override
  String get cdErrTrackNumbers =>
      'The track numbers are not complete or appear twice.';

  @override
  String get cdErrInvalidMeta =>
      'The server did not accept the author, title or numbers.';

  @override
  String get cdErrUnreadableFile =>
      'The server could not read one of the files.';

  @override
  String get cdErrFormatMismatch =>
      'A file does not have the format that the server\'s settings ask for.';

  @override
  String get cdErrBadExtension =>
      'A file has an ending that the server does not accept.';

  @override
  String get cdErrUnknownFile =>
      'The server received a file it did not expect.';

  @override
  String get cdErrMissingFile =>
      'A file is missing in what was sent to the server.';

  @override
  String get cdErrBadCover => 'The cover picture was not accepted.';

  @override
  String get cdErrBadTags =>
      'The tags of a file could not be written by the server.';

  @override
  String get cdErrNoImportFolder =>
      'The server has no import folder for this kind of media.';

  @override
  String get cdErrNoSpace => 'There is not enough room on the server.';

  @override
  String get cdErrDiscExists =>
      'This CD is already in the library; the server does not overwrite it.';

  @override
  String get cdErrTooLarge => 'The upload is larger than the server accepts.';

  @override
  String get cdErrBadRequest => 'The server did not understand the request.';

  @override
  String get cdErrInvalidJob => 'The server did not accept the import job.';

  @override
  String get cdErrJobClosed => 'The import job is already finished.';

  @override
  String get cdErrImportFailed =>
      'The server could not add the CD to the library.';

  @override
  String get cdErrInterrupted =>
      'The import was interrupted, for example by a restart of the server.';

  @override
  String get cdErrStale => 'The import stopped sending news and was given up.';

  @override
  String get cdErrClient => 'The device that started the import gave it up.';

  @override
  String cdErrUnknown(String code) {
    return 'The CD import failed ($code).';
  }

  @override
  String get sectionNew => 'New';

  @override
  String get badgeNew => 'New';

  @override
  String get tooltipPlayed => 'Played';

  @override
  String playsCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Played $count times',
      one: 'Played once',
    );
    return '$_temp0';
  }

  @override
  String lastPlayedOn(DateTime date) {
    final intl.DateFormat dateDateFormat = intl.DateFormat.yMMMd(localeName);
    final String dateString = dateDateFormat.format(date);

    return 'last played on $dateString';
  }

  @override
  String get cdStarting => 'Starting…';

  @override
  String get cdEncoding => 'Converting the audio…';

  @override
  String get cdAddingToLibrary => 'Adding the CD to the library…';

  @override
  String get cdImportingTooltip => 'Importing the CD';

  @override
  String cdEndPresent(String album) {
    return 'This CD is in the library already (ejected): $album';
  }

  @override
  String get cdEndPresentPlain =>
      'This CD is in the library already (ejected).';

  @override
  String get cdEndRejected => 'The CD was rejected. Its files were deleted.';

  @override
  String get cdEndCancelled => 'The import was cancelled.';

  @override
  String get cdWaitsApproval => 'Waits for approval';

  @override
  String get cdWaitsRetry => 'Read error: the CD is still in the drive';

  @override
  String get cdEject => 'Eject';

  @override
  String get cdEjectTooltip => 'Eject CD';

  @override
  String get cdNothingToEject => 'There is nothing to eject.';

  @override
  String get cdErrNoDrive => 'The server has no CD drive.';

  @override
  String get cdErrDriveDenied =>
      'The server may not use its CD drive. The administrator has to allow it.';

  @override
  String get cdErrTrayOpen =>
      'The tray of the drive is open. Close it by hand first.';

  @override
  String get cdErrBusy => 'The drive is busy with another CD.';

  @override
  String get cdErrEjectFailed => 'The drive did not open its tray.';

  @override
  String get cdErrInternal => 'The server had an internal problem.';

  @override
  String get cdErrInvalidSelection =>
      'A chosen track is not on the disc, or is chosen twice.';

  @override
  String get cdSelectAll => 'Select all tracks';

  @override
  String get cdSelectNone => 'Select no track';

  @override
  String get cdSelectInvert => 'Invert the selection';

  @override
  String cdTrackDefault(String number) {
    return 'Track $number';
  }

  @override
  String cdTracksSummary(int selected, int total, String duration) {
    return '$selected of $total tracks, $duration';
  }

  @override
  String get cdErrEmptySelection => 'No track is chosen.';

  @override
  String cdErrTaken(String numbers) {
    return 'Tracks $numbers are already in the library.';
  }

  @override
  String cdBarHead(String title, int disc) {
    return '$title, CD $disc';
  }

  @override
  String cdBarTrack(int track, int total) {
    return 'Track $track of $total';
  }

  @override
  String get cdPhaseReading => 'Reading';

  @override
  String get cdPhaseChecking => 'Checking';

  @override
  String get cdPhaseEncoding => 'Converting';

  @override
  String get cdPhasePlacing => 'Filing';

  @override
  String get cdPhaseImporting => 'Importing';

  @override
  String get cdPhaseWorking => 'Working';

  @override
  String cdBarDone(int count) {
    return 'Done: $count tracks';
  }

  @override
  String get cdBarDoneNoCount => 'Done';

  @override
  String cdBarImporting(String line) {
    return 'Importing: $line';
  }

  @override
  String get cdErrDiscPresent => 'This CD is in the library already.';

  @override
  String get cdErrNotPresent =>
      'This CD is not in the library, so there is nothing to repair.';

  @override
  String get cdErrNothingToHeal =>
      'The CD is whole in the library; there is nothing to repair.';

  @override
  String get cdErrUnreadableTrack =>
      'Some tracks could not be read from the CD (it may be scratched).';

  @override
  String get cdStateUnverified => 'Taken, not confirmed';

  @override
  String get cdErrUnverifiedTrack =>
      'A track was not read the same way twice and was taken with its first reading (not confirmed).';

  @override
  String get cdErrResumeMismatch =>
      'The settings or the choice of tracks changed, so the CD is read again from the start.';

  @override
  String get cdErrHealNeeded =>
      'The CD is in the library, but tracks are missing or damaged. Repair it from the import dialog.';

  @override
  String get cdErrResumePending =>
      'An interrupted reading of this CD is waiting. Continue it from the import dialog.';

  @override
  String get cdErrAmbiguousMatch =>
      'Several releases fit this CD. Please choose one.';

  @override
  String get cdErrNoMatch => 'The online database does not know this CD.';

  @override
  String get cdErrNoTarget =>
      'The server has no import folder that it may write to.';

  @override
  String get cdErrUdevOff =>
      'The automatic import needs the detection of drives by system events to be switched on.';

  @override
  String get cdErrNotAudio => 'This is not an audio CD.';

  @override
  String get cdErrStartFailed => 'The import could not be started.';

  @override
  String get cdErrLocked =>
      'The CD was added, but the import of the library is locked. It shows up when the lock is lifted.';

  @override
  String get cdContinue => 'Continue';

  @override
  String cdResumeNote(int done, int total, String when) {
    return 'This CD was read to $done of $total tracks already (interrupted $when).';
  }

  @override
  String get cdRereadFromStart => 'Read again from the start';

  @override
  String get cdResumeLost =>
      'An earlier reading of this CD cannot be continued, because the settings changed. The CD is read again.';

  @override
  String get cdResumed => 'Continuing an earlier reading.';

  @override
  String get cdImportAnyway => 'Import anyway';

  @override
  String get cdHeal => 'Check and repair';

  @override
  String get cdForceTitle => 'Import the CD again?';

  @override
  String get cdForceBody =>
      'This CD is in the library already. This adds it a second time, as a new copy. Only do that if you really want two copies.';

  @override
  String get cdForceConfirm => 'Import as a new copy';

  @override
  String get cdPresentTitle => 'This CD is in the library already';

  @override
  String cdPresentBookDisc(String book, int disc) {
    return '$book, CD $disc';
  }

  @override
  String cdPresentBookAuthorDisc(String book, String author, int disc) {
    return '$book by $author, CD $disc';
  }

  @override
  String cdPresentComplete(int inLibrary, int onDisc) {
    return 'The CD is whole in the library ($inLibrary of $onDisc tracks).';
  }

  @override
  String cdPresentCount(int inLibrary, int onDisc) {
    return '$inLibrary of $onDisc tracks are in the library.';
  }

  @override
  String get cdMissing => 'Missing';

  @override
  String get cdDamaged => 'Damaged';

  @override
  String get cdDamagedEmpty => 'the file is empty';

  @override
  String get cdDamagedUnreadable => 'the file cannot be read';

  @override
  String cdDamagedLength(String have, String want) {
    return 'the file is $have long, the CD has $want';
  }

  @override
  String get cdDamagedOther => 'the file does not match the CD';

  @override
  String get cdHealHint =>
      'Only the missing and damaged tracks are read from the CD. A damaged file is kept in the quarantine folder.';

  @override
  String get cdStateWaiting => 'Waiting';

  @override
  String get cdStateRead => 'Read, waits for the check';

  @override
  String cdSpeed(String x) {
    return 'Drive reads at ${x}x';
  }

  @override
  String get cdStateRereading => 'Reading again';

  @override
  String cdStateRereadingPercent(int percent) {
    return 'Reading again $percent %';
  }

  @override
  String get cdStateReading => 'Reading';

  @override
  String cdStateReadingPercent(int percent) {
    return 'Reading $percent %';
  }

  @override
  String get cdStateVerifying => 'Checking';

  @override
  String get cdStateEncoding => 'Converting';

  @override
  String get cdStateDone => 'Done';

  @override
  String get cdStateFailed => 'Error';

  @override
  String cdBuffer(int seconds, int max) {
    return 'Buffer $seconds s of $max s';
  }

  @override
  String get filterPlaylists => 'Playlists';

  @override
  String get playlistNew => 'New playlist';

  @override
  String get playlistNewItem => 'New playlist…';

  @override
  String get playlistName => 'Name';

  @override
  String get playlistSave => 'Save';

  @override
  String get playlistsEmpty => 'No playlists yet. Tap + to make one.';

  @override
  String playlistMeta(int count, String duration) {
    return '$count titles · $duration';
  }

  @override
  String get playlistAddTo => 'Add to playlist';

  @override
  String playlistAddedTo(String name) {
    return 'Added to “$name”.';
  }

  @override
  String get playlistChoose => 'Add to which playlist?';

  @override
  String get playlistPlay => 'Play';

  @override
  String get playlistRename => 'Rename';

  @override
  String get playlistRenameTitle => 'Rename the playlist';

  @override
  String get playlistDelete => 'Delete playlist';

  @override
  String get playlistDeleteTitle => 'Delete this playlist?';

  @override
  String playlistDeleteText(String name) {
    return '“$name” is deleted from your profile. The titles stay in the library.';
  }

  @override
  String get playlistRemoveEntry => 'Remove from the playlist';

  @override
  String get playlistUnavailable => 'Not available';

  @override
  String get playlistEmpty =>
      'This playlist is empty. Use “Add to playlist” on a title or a book.';

  @override
  String get playlistDownload => 'Download the playlist';

  @override
  String get plErrNameTaken => 'You have a playlist with this name already.';

  @override
  String get plErrInvalidName =>
      'The name must not be empty and at most 100 characters.';

  @override
  String get plErrUnknownPart =>
      'A title is not there any more, or you may not use it.';

  @override
  String get plErrUnknownBook =>
      'The book is not there any more, or you may not use it.';

  @override
  String get plErrInvalidOrder =>
      'The order could not be used. The list is shown as it is on the server.';

  @override
  String get plErrInvalidPosition =>
      'That place in the playlist does not exist.';

  @override
  String get plErrInvalidRequest => 'The request was not valid.';

  @override
  String get plErrPlaylistFull => 'The playlist is full (at most 5000 titles).';

  @override
  String get plErrTooManyPlaylists =>
      'You have the most playlists you can have (100).';

  @override
  String get plErrNotFound => 'The playlist is not there any more.';

  @override
  String get plErrNetwork => 'The server does not answer.';

  @override
  String plErrUnknown(String code) {
    return 'The playlist could not be changed ($code).';
  }

  @override
  String get hideMenuTooltip => 'Hide or show';

  @override
  String get hideBook => 'Hide this book';

  @override
  String get showBook => 'Show this book again';

  @override
  String hideAuthorAudiobooks(String name) {
    return 'Hide all audiobooks by $name';
  }

  @override
  String showAuthorAudiobooks(String name) {
    return 'Show the audiobooks by $name again';
  }

  @override
  String hideAuthorMusic(String name) {
    return 'Hide all music by $name';
  }

  @override
  String showAuthorMusic(String name) {
    return 'Show the music by $name again';
  }

  @override
  String hideAuthorAll(String name) {
    return 'Hide everything by $name';
  }

  @override
  String showAuthorAll(String name) {
    return 'Show everything by $name again';
  }

  @override
  String get hiddenMark => 'Hidden';

  @override
  String get hidingFailed => 'The change could not be saved.';

  @override
  String get settingsMyLibrary => 'My library';

  @override
  String get settingsShowHidden => 'Show what I hid';

  @override
  String get settingsShowHiddenHint =>
      'What you hide leaves your lists, only yours. With this switch it is listed again, dimmed, so that you can show it again.';

  @override
  String get settingsHiddenTitle => 'Hidden';

  @override
  String get settingsHiddenNone =>
      'You have not hidden anything. Long-press a book, or open it and use the menu at the top.';

  @override
  String settingsHiddenAuthorAudiobooks(String name) {
    return 'Audiobooks by $name';
  }

  @override
  String settingsHiddenAuthorMusic(String name) {
    return 'Music by $name';
  }

  @override
  String settingsHiddenAuthorAll(String name) {
    return 'Everything by $name';
  }

  @override
  String get settingsShowAgain => 'Show again';

  @override
  String musicOf(String name) {
    return 'Music of $name';
  }

  @override
  String get musicOfNobody => 'Other music';

  @override
  String ownerLabel(String name) {
    return 'Owner: $name';
  }

  @override
  String audiobooksOf(String name) {
    return 'Audiobooks of $name';
  }

  @override
  String get audiobooksOfNobody => 'Other audiobooks';

  @override
  String get groupExpand => 'Expand';

  @override
  String get groupCollapse => 'Collapse';

  @override
  String get settingsWebsite => 'Website of the server';

  @override
  String get settingsWebsiteHint =>
      'Opens the start page of the server in the browser, with the way to the administration.';

  @override
  String get settingsWebsiteFailed => 'The browser could not be opened.';

  @override
  String get shareTooltip => 'Share';

  @override
  String shareText(String title, String author) {
    return '$title by $author';
  }

  @override
  String get labelsTitle => 'Labels';

  @override
  String get labelsEdit => 'Edit labels';

  @override
  String get labelsNone => 'No labels yet.';

  @override
  String get trackLabelsEdit => 'Labels of this title';

  @override
  String get labelsAdd => 'Add';

  @override
  String get labelsNew => 'New label';

  @override
  String get labelsSave => 'Save';

  @override
  String get labelsFailed => 'The labels could not be saved.';

  @override
  String get labelsBad =>
      'That is not a usable label: 1 to 40 characters, no comma, semicolon or bar.';

  @override
  String get labelsFilterClear => 'All labels';

  @override
  String get labelsRename => 'Rename label';

  @override
  String get labelsDelete => 'Delete label';

  @override
  String get labelsNoneInLibrary =>
      'No labels in the library yet. The owner of a book sets them on its page.';

  @override
  String labelsDeleteConfirm(String name, int count) {
    return 'Take \"$name\" off $count books?';
  }

  @override
  String get offlineBannerManual =>
      'Offline mode · only downloaded titles are shown';

  @override
  String get offlineBannerUnreachable =>
      'Server not reachable · only downloaded titles are shown';

  @override
  String offlinePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count changes wait for the server',
      one: '1 change waits for the server',
    );
    return '$_temp0';
  }

  @override
  String get offlineRetry => 'Try again';

  @override
  String get offlineNeedsNetwork =>
      'This page needs the server. It is back as soon as you are online.';

  @override
  String get offlineSettingTitle => 'Offline mode';

  @override
  String get offlineSettingHint =>
      'Shows only the titles downloaded on this device and does not ask the server. What you change meanwhile (labels, hidden albums, where you stopped listening) waits and is sent when you switch it off.';

  @override
  String get offlineSyncNow => 'Send changes now';

  @override
  String get offlineSyncing => 'Sending changes …';

  @override
  String get offlineNothingWaits => 'No changes are waiting.';

  @override
  String get offlineRejectedTitle => 'Not accepted by the server';

  @override
  String offlineRejectedLabels(String title, String reason) {
    return 'Labels of “$title” ($reason)';
  }

  @override
  String offlineRejectedHide(String title, String reason) {
    return 'Hiding “$title” ($reason)';
  }

  @override
  String get offlineReasonForbidden => 'not allowed';

  @override
  String get offlineReasonGone => 'no longer there';

  @override
  String get offlineReasonOther => 'refused';

  @override
  String get offlineDismiss => 'OK';

  @override
  String get profileTitle => 'My profile';

  @override
  String get profileSettingsHint => 'Name, picture, language and PIN';

  @override
  String get profilePicture => 'Picture';

  @override
  String get profileTakePhoto => 'Take a photo';

  @override
  String get profileChoosePhoto => 'Choose a picture';

  @override
  String get profileRemovePhotoTitle => 'Remove the picture?';

  @override
  String get profileRemovePhotoAsk =>
      'Your picture is deleted on the server and on all your devices.';

  @override
  String get profileRemovePhoto => 'Remove picture';

  @override
  String get profileNickname => 'Nickname';

  @override
  String get profileFirstName => 'First name';

  @override
  String get profileLastName => 'Last name';

  @override
  String get profileLanguage => 'Language';

  @override
  String get profileLanguageAuto => 'Follow the device';

  @override
  String get profileRole => 'Role';

  @override
  String get roleAdmin => 'Administrator';

  @override
  String get roleMember => 'Member';

  @override
  String get roleChild => 'Child';

  @override
  String get roleGuest => 'Guest';

  @override
  String get profileDevices => 'My devices';

  @override
  String get profileDeviceThis => 'this device';

  @override
  String get profileNeverSeen => 'never seen';

  @override
  String get profileSave => 'Save';

  @override
  String get profileSaved => 'Saved.';

  @override
  String get profilePinTitle => 'Change PIN or password';

  @override
  String get profilePinCurrent => 'Current PIN or password';

  @override
  String get profilePinNew => 'New PIN or password';

  @override
  String get profilePinRepeat => 'Repeat the new one';

  @override
  String get profilePinChanged => 'The PIN has been changed.';

  @override
  String get profileErrOffline =>
      'This needs the server. Try again when you are connected.';

  @override
  String get profileErrExists => 'This nickname is taken.';

  @override
  String get profileErrName => 'This name cannot be used.';

  @override
  String get profileErrPinWrong => 'The current PIN is not right.';

  @override
  String get profileErrPinInvalid => 'The new PIN or password cannot be used.';

  @override
  String get profileErrPinMismatch => 'The two new entries differ.';

  @override
  String get profileErrLocked => 'Too many wrong tries. Try again later.';

  @override
  String profilePinWait(String time) {
    return 'The PIN can be changed again in $time';
  }

  @override
  String get profileErrAvatarType => 'This file is not a picture.';

  @override
  String get profileErrAvatarSize => 'The picture is too large.';

  @override
  String get profileErrCameraDenied =>
      'The camera or the photos may not be used. Allow it in the settings of the system.';

  @override
  String get profileErrCameraNone => 'There is no camera to use here.';

  @override
  String get profileErrGeneric => 'That did not work.';

  @override
  String get profileAvatarTooltip => 'Open your profile';

  @override
  String profileLastSeen(String date) {
    return 'last seen $date';
  }

  @override
  String profileDeviceCount(int used, int max) {
    return 'Devices: $used of $max';
  }

  @override
  String profileDeviceCountFree(int used) {
    return 'Devices: $used';
  }

  @override
  String get episodeDownload => 'Download episode';

  @override
  String get episodeDownloadCancel => 'Stop the download';

  @override
  String get episodeDownloaded => 'On this device';

  @override
  String get episodeRemoveTitle => 'Remove the download?';

  @override
  String get episodeRemoveText =>
      'The episode is deleted from this device. It stays in the podcast and can be downloaded again.';

  @override
  String episodeProgress(String done, String total) {
    return '$done of $total';
  }

  @override
  String get episodeErrorScheme =>
      'This episode has no http or https address and cannot be downloaded.';

  @override
  String get episodeErrorHttp => 'The publisher refused the download.';

  @override
  String episodeErrorTooLarge(String limit) {
    return 'The episode is larger than the limit of $limit in the settings.';
  }

  @override
  String get episodeErrorNetwork =>
      'The download failed: the publisher cannot be reached.';

  @override
  String get episodeErrorIo => 'The file could not be saved on this device.';

  @override
  String podcastDownloadNewest(int count) {
    return 'Download the newest $count unheard episodes';
  }

  @override
  String get podcastDownloadNewestNone => 'Nothing new to download.';

  @override
  String get podcastDownloadsTitle => 'Podcast downloads';

  @override
  String podcastDownloadsUsed(String size, int count) {
    return '$size in $count episodes';
  }

  @override
  String get podcastDownloadsNone => 'No podcast episodes on this device.';

  @override
  String get podcastDownloadsDeleteAll => 'Delete all';

  @override
  String get podcastDownloadsDeleteAllTitle => 'Delete all podcast downloads?';

  @override
  String get podcastDownloadsDeleteAllText =>
      'All downloaded episodes are deleted from this device. The podcasts keep them.';

  @override
  String get podcastDeleteAfter => 'Delete episodes after listening';

  @override
  String get podcastDeleteAfterHint =>
      'A heard episode is deleted from this device, and so is one that the podcast does not list any more. Off: nothing is deleted unless you do it.';

  @override
  String get podcastMaxSize => 'Largest download';

  @override
  String get podcastDownloadAll => 'Download the whole collection';

  @override
  String get podcastDownloadAllTitle => 'Download the whole collection?';

  @override
  String podcastDownloadAllText(int count, String size) {
    return '$count episodes, about $size. They are loaded one after the other and take that much space on this device. You can stop at any time.';
  }

  @override
  String podcastDownloadAllLimited(
    int count,
    String size,
    String limit,
    int fitCount,
    String fitSize,
  ) {
    return 'The collection has $count episodes, about $size. That is more than your limit of $limit, so only the newest $fitCount (about $fitSize) will be loaded. You can raise the limit under Settings > Podcast downloads.';
  }

  @override
  String podcastDownloadAllBlocked(String limit) {
    return 'Even the newest episode is larger than your limit of $limit. You can raise the limit under Settings > Podcast downloads.';
  }

  @override
  String get podcastDownloadAllStart => 'Download';

  @override
  String podcastDownloadAllRunning(int done, int total) {
    return 'Downloading $done of $total ...';
  }

  @override
  String get podcastDownloadAllStop => 'Stop';

  @override
  String podcastDownloadAllDone(int count) {
    return '$count episodes loaded.';
  }

  @override
  String get podcastBulkLimit => 'Largest collection download';

  @override
  String get podcastBulkLimitHint =>
      'The most that \"Download the whole collection\" loads in one go.';

  @override
  String get podcastBulkWifiOnly => 'Collection downloads only on Wi-Fi';

  @override
  String get podcastBulkWifiOnlyHint =>
      '\"Download the whole collection\" does not load on mobile data. A cable or Wi-Fi is fine.';

  @override
  String get podcastDownloadAllMobile =>
      'You are on mobile data. The whole collection is only loaded on Wi-Fi (you can change that under Settings > Podcast downloads).';

  @override
  String podcastDownloadAllSpace(
    String free,
    int count,
    String size,
    int fitCount,
    String fitSize,
  ) {
    return 'Only $free are free on this device, and 1 GB should stay free. Of the $count episodes (about $size) the newest $fitCount (about $fitSize) will be loaded.';
  }

  @override
  String podcastDownloadAllNoSpace(String free) {
    return 'There is not enough room on this device: only $free are free, and 1 GB should stay free. Free some space and try again.';
  }

  @override
  String podcastMegabytes(int mb) {
    return '$mb MB';
  }

  @override
  String get offlinePodcastsNone =>
      'No episodes are on this device yet. Download episodes while you are online to listen to them here.';

  @override
  String get barcodeCheckTooltip => 'Check a barcode';

  @override
  String get barcodeTitle => 'Check a barcode';

  @override
  String get barcodeFieldLabel => 'Barcode';

  @override
  String get barcodeCheck => 'Check';

  @override
  String get barcodeInLibrary => 'In the library:';

  @override
  String barcodeNotInLibrary(String code) {
    return 'Not in the library: $code';
  }

  @override
  String get barcodeInvalid => 'This is not a valid barcode. Check the digits.';

  @override
  String get barcodeOfflineNotFound =>
      'The library on this device does not have it. The server was not asked, because you are offline.';

  @override
  String get barcodeLookupFailed =>
      'The server could not be asked. Try again later.';

  @override
  String get barcodeOpen => 'Open';

  @override
  String get barcodeScanHint =>
      'Hold the barcode of the CD in front of the camera';

  @override
  String get barcodeTypeHint => 'Or type the digits below the barcode:';

  @override
  String get inboxTitle => 'CD inbox';

  @override
  String get inboxNotificationTitle => 'CDs';

  @override
  String inboxCardWaiting(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count CDs wait for approval',
      one: '1 CD waits for approval',
    );
    return '$_temp0';
  }

  @override
  String inboxCardUnread(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count unread',
      one: '1 unread',
    );
    return '$_temp0';
  }

  @override
  String get inboxOpen => 'Open the inbox';

  @override
  String get inboxEmpty => 'No messages about your CDs.';

  @override
  String get inboxMarkAllRead => 'Mark all as read';

  @override
  String get inboxImport => 'Import';

  @override
  String get inboxReject => 'Reject';

  @override
  String get cdGiveUp => 'Give up';

  @override
  String get cdGiveUpTitle => 'Give up on this CD?';

  @override
  String get cdGiveUpText =>
      'The import ends. What was read is kept for a while, so reading the CD again later is quicker.';

  @override
  String get inboxRetry => 'Try again';

  @override
  String get inboxOpenAlbum => 'Open album';

  @override
  String get inboxRejectTitle => 'Reject this CD?';

  @override
  String get inboxRejectText => 'Its files are deleted.';

  @override
  String get inboxWaitsForApproval => 'Waits for approval';

  @override
  String get inboxWorking => 'One moment…';

  @override
  String inboxResolvedApproved(String by) {
    return 'Imported by $by';
  }

  @override
  String inboxResolvedRejected(String by) {
    return 'Rejected by $by';
  }

  @override
  String get inboxResolvedApprovedYou => 'Imported';

  @override
  String get inboxResolvedRejectedYou => 'Rejected';

  @override
  String get inboxResolvedExpired => 'Expired';

  @override
  String inboxAlreadyDecided(String by) {
    return 'Already decided by $by';
  }

  @override
  String get inboxNeedsServer =>
      'The inbox needs the server. It is back as soon as you are online.';

  @override
  String get inboxLoadFailed => 'The inbox could not be loaded.';

  @override
  String get inboxErrNetwork => 'The server could not be reached.';

  @override
  String get inboxErrForbidden => 'You may not decide CDs.';

  @override
  String get inboxErrBusy => 'The drive is busy.';

  @override
  String get inboxErrNoDisc => 'There is no CD in the drive.';

  @override
  String get inboxErrDrive => 'The drive cannot be used now.';

  @override
  String get inboxErrGeneric => 'That did not work.';

  @override
  String get inboxUnknown =>
      'News from the server that this version of the app does not know yet.';

  @override
  String get inboxThisCd => 'a CD';

  @override
  String inboxDiscOf(int disc, int total) {
    return 'CD $disc of $total';
  }

  @override
  String inboxDiscNumber(int disc) {
    return 'CD $disc';
  }

  @override
  String inboxCdAwaitingApproval(String by, String album) {
    return 'New CD from $by: $album. Import it?';
  }

  @override
  String inboxCdAwaitingApprovalAuto(String album) {
    return 'New CD put in at the server: $album. Import it?';
  }

  @override
  String inboxCdWaiting(String album) {
    return '$album waits for approval.';
  }

  @override
  String inboxCdImported(String album, int tracks) {
    String _temp0 = intl.Intl.pluralLogic(
      tracks,
      locale: localeName,
      other: '$tracks tracks',
      one: '1 track',
    );
    return '$album is in the library ($_temp0).';
  }

  @override
  String inboxCdImportedNoCount(String album) {
    return '$album is in the library.';
  }

  @override
  String inboxCdPresent(String album) {
    return '$album is in the library already. The CD was not read.';
  }

  @override
  String inboxCdDecidedApproved(String by, String album) {
    return '$by imported the CD $album.';
  }

  @override
  String inboxCdDecidedRejected(String by, String album) {
    return '$by rejected the CD $album.';
  }

  @override
  String inboxCdExpired(String album) {
    return 'The CD $album was not decided in time. Its files were deleted.';
  }

  @override
  String inboxCdReadError(String album, String tracks) {
    return '$album: could not read track $tracks.';
  }

  @override
  String inboxCdReadErrorNoTracks(String album) {
    return '$album: some tracks could not be read.';
  }

  @override
  String get inboxCdNoMatch =>
      'The CD in the drive is not known to MusicBrainz. Choose it in the web panel.';

  @override
  String inboxCdAmbiguous(int candidates) {
    String _temp0 = intl.Intl.pluralLogic(
      candidates,
      locale: localeName,
      other: '$candidates releases',
      one: '1 release',
    );
    return 'The CD fits $_temp0. Choose the right one in the web panel.';
  }

  @override
  String get inboxCdAmbiguousNoCount =>
      'The CD fits several releases. Choose the right one in the web panel.';

  @override
  String inboxCdHealNeeded(String album) {
    return '$album is in the library only in part. It must be repaired before the CD is read.';
  }

  @override
  String inboxCdNoSpace(String album) {
    return 'There is not enough free space to read $album.';
  }

  @override
  String inboxCdNoTarget(String album) {
    return '$album was not read: no folder for imported CDs is set up.';
  }

  @override
  String get inboxAlbumNotFound =>
      'The album is not in the library of this device yet. Refresh the library.';

  @override
  String get cdErrInvalidMove =>
      'This CD is not in the state the step needs. The list is read again.';

  @override
  String get cdErrDecided => 'Somebody else was faster and decided this CD.';

  @override
  String get cdErrInvalidPaging =>
      'The page of the inbox asked for does not exist.';
}
