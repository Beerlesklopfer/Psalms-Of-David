import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_de.dart';
import 'app_localizations_en.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('de'),
    Locale('en'),
  ];

  /// No description provided for @appTitle.
  ///
  /// In en, this message translates to:
  /// **'Psalms'**
  String get appTitle;

  /// No description provided for @navHome.
  ///
  /// In en, this message translates to:
  /// **'Home'**
  String get navHome;

  /// No description provided for @navSearch.
  ///
  /// In en, this message translates to:
  /// **'Search'**
  String get navSearch;

  /// No description provided for @navLibrary.
  ///
  /// In en, this message translates to:
  /// **'Library'**
  String get navLibrary;

  /// No description provided for @greetingMorning.
  ///
  /// In en, this message translates to:
  /// **'Good morning'**
  String get greetingMorning;

  /// No description provided for @greetingDay.
  ///
  /// In en, this message translates to:
  /// **'Good afternoon'**
  String get greetingDay;

  /// No description provided for @greetingEvening.
  ///
  /// In en, this message translates to:
  /// **'Good evening'**
  String get greetingEvening;

  /// No description provided for @sectionContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue listening'**
  String get sectionContinue;

  /// No description provided for @sectionYourBooks.
  ///
  /// In en, this message translates to:
  /// **'Your audiobooks'**
  String get sectionYourBooks;

  /// No description provided for @sectionAuthors.
  ///
  /// In en, this message translates to:
  /// **'Authors'**
  String get sectionAuthors;

  /// No description provided for @sectionChapters.
  ///
  /// In en, this message translates to:
  /// **'Chapters'**
  String get sectionChapters;

  /// No description provided for @discLabel.
  ///
  /// In en, this message translates to:
  /// **'CD {number}'**
  String discLabel(int number);

  /// No description provided for @discTrack.
  ///
  /// In en, this message translates to:
  /// **'CD {disc} · {title}'**
  String discTrack(int disc, String title);

  /// No description provided for @bookMeta.
  ///
  /// In en, this message translates to:
  /// **'{discs, plural, =1{1 CD} other{{discs} CDs}} · {tracks, plural, =1{1 track} other{{tracks} tracks}} · {duration}'**
  String bookMeta(int discs, int tracks, String duration);

  /// No description provided for @durationHoursMinutes.
  ///
  /// In en, this message translates to:
  /// **'{hours} h {minutes} min'**
  String durationHoursMinutes(int hours, int minutes);

  /// No description provided for @durationMinutes.
  ///
  /// In en, this message translates to:
  /// **'{minutes} min'**
  String durationMinutes(int minutes);

  /// No description provided for @percentListened.
  ///
  /// In en, this message translates to:
  /// **'{percent}% listened'**
  String percentListened(int percent);

  /// No description provided for @actionListen.
  ///
  /// In en, this message translates to:
  /// **'Listen'**
  String get actionListen;

  /// No description provided for @actionContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get actionContinue;

  /// No description provided for @tooltipPlay.
  ///
  /// In en, this message translates to:
  /// **'Play'**
  String get tooltipPlay;

  /// No description provided for @tooltipPause.
  ///
  /// In en, this message translates to:
  /// **'Pause'**
  String get tooltipPause;

  /// No description provided for @tooltipClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get tooltipClose;

  /// No description provided for @tooltipClear.
  ///
  /// In en, this message translates to:
  /// **'Clear'**
  String get tooltipClear;

  /// No description provided for @tooltipPrevious.
  ///
  /// In en, this message translates to:
  /// **'Previous track'**
  String get tooltipPrevious;

  /// No description provided for @tooltipNext.
  ///
  /// In en, this message translates to:
  /// **'Next track'**
  String get tooltipNext;

  /// No description provided for @tooltipBack10.
  ///
  /// In en, this message translates to:
  /// **'Back 10 seconds'**
  String get tooltipBack10;

  /// No description provided for @tooltipForward30.
  ///
  /// In en, this message translates to:
  /// **'Forward 30 seconds'**
  String get tooltipForward30;

  /// No description provided for @tooltipDownloadUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Download (not available yet)'**
  String get tooltipDownloadUnavailable;

  /// No description provided for @nothingSelected.
  ///
  /// In en, this message translates to:
  /// **'Nothing selected'**
  String get nothingSelected;

  /// No description provided for @searchHint.
  ///
  /// In en, this message translates to:
  /// **'Title or author'**
  String get searchHint;

  /// No description provided for @nothingFound.
  ///
  /// In en, this message translates to:
  /// **'Nothing found'**
  String get nothingFound;

  /// No description provided for @filterBooks.
  ///
  /// In en, this message translates to:
  /// **'Audiobooks'**
  String get filterBooks;

  /// No description provided for @filterMusic.
  ///
  /// In en, this message translates to:
  /// **'Music'**
  String get filterMusic;

  /// No description provided for @noMusicYet.
  ///
  /// In en, this message translates to:
  /// **'No music in the library yet'**
  String get noMusicYet;

  /// No description provided for @connectTitle.
  ///
  /// In en, this message translates to:
  /// **'Connect to your server'**
  String get connectTitle;

  /// No description provided for @connectIntro.
  ///
  /// In en, this message translates to:
  /// **'Open the administration page of your server, create a pairing code and paste the address below it here.'**
  String get connectIntro;

  /// No description provided for @pairingAddressLabel.
  ///
  /// In en, this message translates to:
  /// **'Pairing address'**
  String get pairingAddressLabel;

  /// No description provided for @deviceNameLabel.
  ///
  /// In en, this message translates to:
  /// **'Name of this device'**
  String get deviceNameLabel;

  /// No description provided for @deviceNameGenericHint.
  ///
  /// In en, this message translates to:
  /// **'iOS tells the app only the model. Give this device a name you will recognise, for example \"Anna\'s iPad\".'**
  String get deviceNameGenericHint;

  /// No description provided for @connectButton.
  ///
  /// In en, this message translates to:
  /// **'Connect'**
  String get connectButton;

  /// No description provided for @retryButton.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get retryButton;

  /// No description provided for @useDemoButton.
  ///
  /// In en, this message translates to:
  /// **'Try with demo data'**
  String get useDemoButton;

  /// No description provided for @connectErrorAddress.
  ///
  /// In en, this message translates to:
  /// **'This is not a pairing address.'**
  String get connectErrorAddress;

  /// No description provided for @connectErrorCode.
  ///
  /// In en, this message translates to:
  /// **'The code is invalid, used or expired. Create a new one.'**
  String get connectErrorCode;

  /// No description provided for @connectErrorLimit.
  ///
  /// In en, this message translates to:
  /// **'This user has reached the limit of devices.'**
  String get connectErrorLimit;

  /// No description provided for @connectErrorNetwork.
  ///
  /// In en, this message translates to:
  /// **'The server cannot be reached.'**
  String get connectErrorNetwork;

  /// No description provided for @connectErrorCertificate.
  ///
  /// In en, this message translates to:
  /// **'The server shows a different certificate than the one in the code.'**
  String get connectErrorCertificate;

  /// No description provided for @connectErrorSignedOut.
  ///
  /// In en, this message translates to:
  /// **'The server no longer knows this device. Pair it again with a new code; the old pairing stays until then.'**
  String get connectErrorSignedOut;

  /// No description provided for @tooltipTimeBook.
  ///
  /// In en, this message translates to:
  /// **'Time of the whole CD. Tap for the time of this track.'**
  String get tooltipTimeBook;

  /// No description provided for @tooltipTimeTrack.
  ///
  /// In en, this message translates to:
  /// **'Time of this track. Tap for the time of the whole CD.'**
  String get tooltipTimeTrack;

  /// No description provided for @formatNotPlayableIos.
  ///
  /// In en, this message translates to:
  /// **'This file format cannot be played on Apple devices (iPhone, iPad, Mac) yet.'**
  String get formatNotPlayableIos;

  /// No description provided for @connectSearching.
  ///
  /// In en, this message translates to:
  /// **'Looking for the server again ...'**
  String get connectSearching;

  /// No description provided for @connectErrorOther.
  ///
  /// In en, this message translates to:
  /// **'Connecting failed.'**
  String get connectErrorOther;

  /// No description provided for @openPassage.
  ///
  /// In en, this message translates to:
  /// **'Open passage “{text}”'**
  String openPassage(String text);

  /// No description provided for @passageNotFound.
  ///
  /// In en, this message translates to:
  /// **'No recording has this passage for you right now.'**
  String get passageNotFound;

  /// No description provided for @loadingFile.
  ///
  /// In en, this message translates to:
  /// **'Loading…'**
  String get loadingFile;

  /// No description provided for @pasteButton.
  ///
  /// In en, this message translates to:
  /// **'Paste'**
  String get pasteButton;

  /// No description provided for @qrButton.
  ///
  /// In en, this message translates to:
  /// **'Scan the QR code'**
  String get qrButton;

  /// No description provided for @qrScanTitle.
  ///
  /// In en, this message translates to:
  /// **'Scan the pairing code'**
  String get qrScanTitle;

  /// No description provided for @qrScanHint.
  ///
  /// In en, this message translates to:
  /// **'Hold the camera on the QR code of the pairing page of your server.'**
  String get qrScanHint;

  /// No description provided for @qrScanNotPairing.
  ///
  /// In en, this message translates to:
  /// **'This is not a pairing code.'**
  String get qrScanNotPairing;

  /// No description provided for @qrScanUsePicture.
  ///
  /// In en, this message translates to:
  /// **'Choose a picture instead'**
  String get qrScanUsePicture;

  /// No description provided for @qrScanTorch.
  ///
  /// In en, this message translates to:
  /// **'Torch'**
  String get qrScanTorch;

  /// No description provided for @qrScanPermissionDenied.
  ///
  /// In en, this message translates to:
  /// **'The camera is not allowed for Psalms. Allow it in the settings, or choose a picture of the QR code instead.'**
  String get qrScanPermissionDenied;

  /// No description provided for @qrScanOpenSettings.
  ///
  /// In en, this message translates to:
  /// **'Open settings'**
  String get qrScanOpenSettings;

  /// No description provided for @qrNotFound.
  ///
  /// In en, this message translates to:
  /// **'No QR code found in this picture. Try a sharper or larger one.'**
  String get qrNotFound;

  /// No description provided for @qrChooseTitle.
  ///
  /// In en, this message translates to:
  /// **'Choose a picture of the QR code'**
  String get qrChooseTitle;

  /// No description provided for @bookPlayed.
  ///
  /// In en, this message translates to:
  /// **'Book: {time} played'**
  String bookPlayed(String time);

  /// No description provided for @bookTotal.
  ///
  /// In en, this message translates to:
  /// **'of {time}'**
  String bookTotal(String time);

  /// No description provided for @navSettings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get navSettings;

  /// No description provided for @navAbout.
  ///
  /// In en, this message translates to:
  /// **'About'**
  String get navAbout;

  /// No description provided for @settingsServer.
  ///
  /// In en, this message translates to:
  /// **'Server'**
  String get settingsServer;

  /// No description provided for @settingsDemo.
  ///
  /// In en, this message translates to:
  /// **'This is the demo library. No server is connected.'**
  String get settingsDemo;

  /// No description provided for @settingsAddress.
  ///
  /// In en, this message translates to:
  /// **'Address'**
  String get settingsAddress;

  /// No description provided for @settingsDevice.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get settingsDevice;

  /// No description provided for @settingsDisconnect.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get settingsDisconnect;

  /// No description provided for @settingsGroupGeneral.
  ///
  /// In en, this message translates to:
  /// **'General'**
  String get settingsGroupGeneral;

  /// No description provided for @settingsGroupOffline.
  ///
  /// In en, this message translates to:
  /// **'Offline and downloads'**
  String get settingsGroupOffline;

  /// No description provided for @settingsGroupLibrary.
  ///
  /// In en, this message translates to:
  /// **'Library'**
  String get settingsGroupLibrary;

  /// No description provided for @settingsGroupDevices.
  ///
  /// In en, this message translates to:
  /// **'Further devices'**
  String get settingsGroupDevices;

  /// No description provided for @settingsGroupServer.
  ///
  /// In en, this message translates to:
  /// **'Server'**
  String get settingsGroupServer;

  /// No description provided for @sectionHeardBy.
  ///
  /// In en, this message translates to:
  /// **'Recently heard by {name}'**
  String sectionHeardBy(String name);

  /// No description provided for @sectionMyMediaTime.
  ///
  /// In en, this message translates to:
  /// **'My media time'**
  String get sectionMyMediaTime;

  /// No description provided for @sectionMediaTimeOf.
  ///
  /// In en, this message translates to:
  /// **'Media time of {name}'**
  String sectionMediaTimeOf(String name);

  /// No description provided for @chartTotal.
  ///
  /// In en, this message translates to:
  /// **'Total'**
  String get chartTotal;

  /// No description provided for @chartNothing.
  ///
  /// In en, this message translates to:
  /// **'Nothing listened to in the last days.'**
  String get chartNothing;

  /// No description provided for @chartMinutes.
  ///
  /// In en, this message translates to:
  /// **'{n} min'**
  String chartMinutes(int n);

  /// No description provided for @chartHours.
  ///
  /// In en, this message translates to:
  /// **'{n} h'**
  String chartHours(int n);

  /// No description provided for @settingsShareListening.
  ///
  /// In en, this message translates to:
  /// **'Show what I listen to'**
  String get settingsShareListening;

  /// No description provided for @settingsShareListeningHint.
  ///
  /// In en, this message translates to:
  /// **'The family sees your recently heard books on their start page.'**
  String get settingsShareListeningHint;

  /// No description provided for @settingsShareListeningLocked.
  ///
  /// In en, this message translates to:
  /// **'An administrator decided that your listening is visible to the family.'**
  String get settingsShareListeningLocked;

  /// No description provided for @settingsShareListeningFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not change this setting.'**
  String get settingsShareListeningFailed;

  /// No description provided for @settingsTheme.
  ///
  /// In en, this message translates to:
  /// **'Colours'**
  String get settingsTheme;

  /// No description provided for @themeSystem.
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get themeSystem;

  /// No description provided for @themeLight.
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get themeLight;

  /// No description provided for @themeDark.
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get themeDark;

  /// No description provided for @settingsDisconnectHint.
  ///
  /// In en, this message translates to:
  /// **'Forgets the pairing with the server on this device. To connect again you need a new pairing code.'**
  String get settingsDisconnectHint;

  /// No description provided for @settingsPlayback.
  ///
  /// In en, this message translates to:
  /// **'Playback'**
  String get settingsPlayback;

  /// No description provided for @settingsBuffer.
  ///
  /// In en, this message translates to:
  /// **'Audio buffer'**
  String get settingsBuffer;

  /// No description provided for @settingsBufferSeconds.
  ///
  /// In en, this message translates to:
  /// **'{seconds} s'**
  String settingsBufferSeconds(int seconds);

  /// No description provided for @settingsBufferHint.
  ///
  /// In en, this message translates to:
  /// **'How much audio is kept ready before it is played. A larger buffer bridges a slow network or disk, a smaller one lets a change of track be heard sooner. It applies from the next track.'**
  String get settingsBufferHint;

  /// No description provided for @settingsDevices.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get settingsDevices;

  /// No description provided for @settingsDevicesCount.
  ///
  /// In en, this message translates to:
  /// **'{used} of {max} devices connected'**
  String settingsDevicesCount(int used, int max);

  /// No description provided for @settingsDevicesNoLimit.
  ///
  /// In en, this message translates to:
  /// **'{used} devices connected'**
  String settingsDevicesNoLimit(int used);

  /// No description provided for @settingsConnectDevice.
  ///
  /// In en, this message translates to:
  /// **'Connect another device'**
  String get settingsConnectDevice;

  /// No description provided for @settingsConnectDeviceHint.
  ///
  /// In en, this message translates to:
  /// **'Shows a QR code that the other device scans. The code works once and only for a short time.'**
  String get settingsConnectDeviceHint;

  /// No description provided for @pairQrScan.
  ///
  /// In en, this message translates to:
  /// **'Scan this code with Psalms on the other device.'**
  String get pairQrScan;

  /// No description provided for @pairQrExpires.
  ///
  /// In en, this message translates to:
  /// **'Valid for {time}'**
  String pairQrExpires(String time);

  /// No description provided for @pairQrExpired.
  ///
  /// In en, this message translates to:
  /// **'The code has expired.'**
  String get pairQrExpired;

  /// No description provided for @pairQrNew.
  ///
  /// In en, this message translates to:
  /// **'New code'**
  String get pairQrNew;

  /// No description provided for @pairQrFull.
  ///
  /// In en, this message translates to:
  /// **'All devices are connected. Remove one in the administration of the server to connect another.'**
  String get pairQrFull;

  /// No description provided for @pairQrError.
  ///
  /// In en, this message translates to:
  /// **'The code could not be made. Try again.'**
  String get pairQrError;

  /// No description provided for @pairQrClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get pairQrClose;

  /// No description provided for @pairQrCopy.
  ///
  /// In en, this message translates to:
  /// **'Copy the address'**
  String get pairQrCopy;

  /// No description provided for @pairQrCopied.
  ///
  /// In en, this message translates to:
  /// **'Address copied. Paste it into the connect screen of the other device.'**
  String get pairQrCopied;

  /// No description provided for @streamArtist.
  ///
  /// In en, this message translates to:
  /// **'Artist'**
  String get streamArtist;

  /// No description provided for @streamTrack.
  ///
  /// In en, this message translates to:
  /// **'Track'**
  String get streamTrack;

  /// No description provided for @streamStation.
  ///
  /// In en, this message translates to:
  /// **'Station'**
  String get streamStation;

  /// No description provided for @streamGenre.
  ///
  /// In en, this message translates to:
  /// **'Genre'**
  String get streamGenre;

  /// No description provided for @streamDescription.
  ///
  /// In en, this message translates to:
  /// **'About'**
  String get streamDescription;

  /// No description provided for @streamQuality.
  ///
  /// In en, this message translates to:
  /// **'Quality'**
  String get streamQuality;

  /// No description provided for @streamWebsite.
  ///
  /// In en, this message translates to:
  /// **'Website'**
  String get streamWebsite;

  /// No description provided for @streamWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for the information of the station …'**
  String get streamWaiting;

  /// No description provided for @cancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get cancel;

  /// No description provided for @aboutVersion.
  ///
  /// In en, this message translates to:
  /// **'Version {version}'**
  String aboutVersion(String version);

  /// No description provided for @aboutDescription.
  ///
  /// In en, this message translates to:
  /// **'Psalms plays audiobooks, music and more from your own David server, for the whole family.'**
  String get aboutDescription;

  /// No description provided for @aboutLicense.
  ///
  /// In en, this message translates to:
  /// **'Free software under the GNU Affero General Public License, version 3 or later.'**
  String get aboutLicense;

  /// No description provided for @aboutLicenses.
  ///
  /// In en, this message translates to:
  /// **'Open-source licences'**
  String get aboutLicenses;

  /// No description provided for @tooltipSpeed.
  ///
  /// In en, this message translates to:
  /// **'Playback speed'**
  String get tooltipSpeed;

  /// No description provided for @aboutLicenseTitle.
  ///
  /// In en, this message translates to:
  /// **'License'**
  String get aboutLicenseTitle;

  /// No description provided for @aboutScripture.
  ///
  /// In en, this message translates to:
  /// **'Scripture'**
  String get aboutScripture;

  /// No description provided for @aboutFamily.
  ///
  /// In en, this message translates to:
  /// **'Family'**
  String get aboutFamily;

  /// No description provided for @connectNeedsDavid.
  ///
  /// In en, this message translates to:
  /// **'Psalms is only the player. It needs a David server, which holds your audiobooks, music and the family\'s profiles. Without one, Psalms has nothing to play.'**
  String get connectNeedsDavid;

  /// No description provided for @aboutDavidTitle.
  ///
  /// In en, this message translates to:
  /// **'Psalms needs a David server'**
  String get aboutDavidTitle;

  /// No description provided for @aboutDavidText.
  ///
  /// In en, this message translates to:
  /// **'Psalms does not come with any content. It plays what a David server offers: audiobooks, music and more from your own library, with a profile for each family member. David is free software that you run yourself, on a home server or a small computer. Whoever runs it creates a pairing code on its admin page; you enter that code here once. Without a David server you can only try the demo data.'**
  String get aboutDavidText;

  /// No description provided for @aboutDavidSteps.
  ///
  /// In en, this message translates to:
  /// **'1. Run a David server.  2. Open its admin page and choose Pair a device.  3. Scan or paste the code in Psalms.'**
  String get aboutDavidSteps;

  /// No description provided for @tooltipRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh the library'**
  String get tooltipRefresh;

  /// No description provided for @tooltipDownload.
  ///
  /// In en, this message translates to:
  /// **'Download for listening without the server'**
  String get tooltipDownload;

  /// No description provided for @tooltipDownloadCancel.
  ///
  /// In en, this message translates to:
  /// **'Stop the download'**
  String get tooltipDownloadCancel;

  /// No description provided for @tooltipDownloaded.
  ///
  /// In en, this message translates to:
  /// **'On this device. Tap to remove.'**
  String get tooltipDownloaded;

  /// No description provided for @downloadedCount.
  ///
  /// In en, this message translates to:
  /// **'{done} of {total}'**
  String downloadedCount(int done, int total);

  /// No description provided for @downloadRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove the download?'**
  String get downloadRemoveTitle;

  /// No description provided for @downloadRemoveText.
  ///
  /// In en, this message translates to:
  /// **'The files are deleted from this device. The book stays on the server and can be downloaded again.'**
  String get downloadRemoveText;

  /// No description provided for @downloadRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get downloadRemove;

  /// No description provided for @aboutStorage.
  ///
  /// In en, this message translates to:
  /// **'Storage on this device'**
  String get aboutStorage;

  /// No description provided for @aboutStorageNone.
  ///
  /// In en, this message translates to:
  /// **'No downloads'**
  String get aboutStorageNone;

  /// No description provided for @aboutStorageUsed.
  ///
  /// In en, this message translates to:
  /// **'{size} in {count} downloaded parts'**
  String aboutStorageUsed(String size, int count);

  /// No description provided for @aboutStorageRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove all downloads?'**
  String get aboutStorageRemoveTitle;

  /// No description provided for @craTitle.
  ///
  /// In en, this message translates to:
  /// **'Security information (EU Cyber Resilience Act)'**
  String get craTitle;

  /// No description provided for @craManufacturer.
  ///
  /// In en, this message translates to:
  /// **'Manufacturer'**
  String get craManufacturer;

  /// No description provided for @craVersion.
  ///
  /// In en, this message translates to:
  /// **'Version and licence'**
  String get craVersion;

  /// No description provided for @craContact.
  ///
  /// In en, this message translates to:
  /// **'Report a security problem to'**
  String get craContact;

  /// No description provided for @craSupport.
  ///
  /// In en, this message translates to:
  /// **'Security updates'**
  String get craSupport;

  /// No description provided for @craSupportOpen.
  ///
  /// In en, this message translates to:
  /// **'Given with new versions. The end of the support period has not been set yet.'**
  String get craSupportOpen;

  /// No description provided for @craSupportUntil.
  ///
  /// In en, this message translates to:
  /// **'Until {date}'**
  String craSupportUntil(String date);

  /// No description provided for @craData.
  ///
  /// In en, this message translates to:
  /// **'Data and network'**
  String get craData;

  /// No description provided for @craDataText.
  ///
  /// In en, this message translates to:
  /// **'Psalms talks only to the David server you paired it with, over a connection that checks the server\'s certificate. It has no analytics and no advertising. The pairing and the downloaded files stay on this device.'**
  String get craDataText;

  /// No description provided for @craComponents.
  ///
  /// In en, this message translates to:
  /// **'Software components'**
  String get craComponents;

  /// No description provided for @craComponentsText.
  ///
  /// In en, this message translates to:
  /// **'The list of all components with their licences.'**
  String get craComponentsText;

  /// No description provided for @aboutSource.
  ///
  /// In en, this message translates to:
  /// **'Source code'**
  String get aboutSource;

  /// No description provided for @filterVideos.
  ///
  /// In en, this message translates to:
  /// **'Videos'**
  String get filterVideos;

  /// No description provided for @filterEbooks.
  ///
  /// In en, this message translates to:
  /// **'E-Books'**
  String get filterEbooks;

  /// No description provided for @comingSoonTitle.
  ///
  /// In en, this message translates to:
  /// **'Not available yet'**
  String get comingSoonTitle;

  /// No description provided for @comingSoonVideos.
  ///
  /// In en, this message translates to:
  /// **'Videos are planned. David does not manage them yet, so there is nothing to show here.'**
  String get comingSoonVideos;

  /// No description provided for @comingSoonEbooks.
  ///
  /// In en, this message translates to:
  /// **'E-books and PDF files are planned. David does not manage them yet, so there is nothing to show here.'**
  String get comingSoonEbooks;

  /// No description provided for @noAudiobooksYet.
  ///
  /// In en, this message translates to:
  /// **'No audiobooks in the library yet'**
  String get noAudiobooksYet;

  /// No description provided for @readBy.
  ///
  /// In en, this message translates to:
  /// **'Read by {name}'**
  String readBy(String name);

  /// No description provided for @filterAll.
  ///
  /// In en, this message translates to:
  /// **'All'**
  String get filterAll;

  /// No description provided for @sectionArtists.
  ///
  /// In en, this message translates to:
  /// **'Artists'**
  String get sectionArtists;

  /// No description provided for @sectionAuthorsAndArtists.
  ///
  /// In en, this message translates to:
  /// **'Authors and artists'**
  String get sectionAuthorsAndArtists;

  /// No description provided for @artistLabel.
  ///
  /// In en, this message translates to:
  /// **'Artist: {name}'**
  String artistLabel(String name);

  /// No description provided for @sectionYourMusic.
  ///
  /// In en, this message translates to:
  /// **'Your music'**
  String get sectionYourMusic;

  /// No description provided for @craEmail.
  ///
  /// In en, this message translates to:
  /// **'Contact'**
  String get craEmail;

  /// No description provided for @craWebsite.
  ///
  /// In en, this message translates to:
  /// **'Website'**
  String get craWebsite;

  /// No description provided for @greetingNamed.
  ///
  /// In en, this message translates to:
  /// **'{greeting}, {name}'**
  String greetingNamed(String greeting, String name);

  /// No description provided for @sectionRecent.
  ///
  /// In en, this message translates to:
  /// **'Recently listened'**
  String get sectionRecent;

  /// No description provided for @genreOther.
  ///
  /// In en, this message translates to:
  /// **'Other'**
  String get genreOther;

  /// No description provided for @sectionAlbums.
  ///
  /// In en, this message translates to:
  /// **'Albums'**
  String get sectionAlbums;

  /// No description provided for @sectionBooks.
  ///
  /// In en, this message translates to:
  /// **'Books'**
  String get sectionBooks;

  /// No description provided for @sectionNarrators.
  ///
  /// In en, this message translates to:
  /// **'Narrators'**
  String get sectionNarrators;

  /// No description provided for @sectionAlbumsAndBooks.
  ///
  /// In en, this message translates to:
  /// **'Albums and books'**
  String get sectionAlbumsAndBooks;

  /// No description provided for @navPodcasts.
  ///
  /// In en, this message translates to:
  /// **'Podcasts'**
  String get navPodcasts;

  /// No description provided for @navRadio.
  ///
  /// In en, this message translates to:
  /// **'Radio'**
  String get navRadio;

  /// No description provided for @podcastAdd.
  ///
  /// In en, this message translates to:
  /// **'Follow a podcast'**
  String get podcastAdd;

  /// No description provided for @podcastAddress.
  ///
  /// In en, this message translates to:
  /// **'Address of the feed or the web page'**
  String get podcastAddress;

  /// No description provided for @podcastFollow.
  ///
  /// In en, this message translates to:
  /// **'Follow'**
  String get podcastFollow;

  /// No description provided for @podcastFollowed.
  ///
  /// In en, this message translates to:
  /// **'Now following “{title}”'**
  String podcastFollowed(String title);

  /// No description provided for @askToRelease.
  ///
  /// In en, this message translates to:
  /// **'Please ask to have it released. It waits until a parent allows it.'**
  String get askToRelease;

  /// No description provided for @podcastInvalid.
  ///
  /// In en, this message translates to:
  /// **'That address gives no podcast feed.'**
  String get podcastInvalid;

  /// No description provided for @notAllowed.
  ///
  /// In en, this message translates to:
  /// **'You are not allowed to do that.'**
  String get notAllowed;

  /// No description provided for @serverUnreachableShort.
  ///
  /// In en, this message translates to:
  /// **'The server cannot be reached.'**
  String get serverUnreachableShort;

  /// No description provided for @needsServer.
  ///
  /// In en, this message translates to:
  /// **'Podcasts and radio need a connection to your server.'**
  String get needsServer;

  /// No description provided for @noPodcastsYet.
  ///
  /// In en, this message translates to:
  /// **'No podcasts yet. Tap + to follow one.'**
  String get noPodcastsYet;

  /// No description provided for @waitingForRelease.
  ///
  /// In en, this message translates to:
  /// **'Waiting for a release. Please ask to have it released.'**
  String get waitingForRelease;

  /// No description provided for @feedProblem.
  ///
  /// In en, this message translates to:
  /// **'The feed could not be read the last time.'**
  String get feedProblem;

  /// No description provided for @podcastUnfollowTitle.
  ///
  /// In en, this message translates to:
  /// **'Stop following this podcast?'**
  String get podcastUnfollowTitle;

  /// No description provided for @podcastUnfollowAsk.
  ///
  /// In en, this message translates to:
  /// **'\"{title}\" disappears from the podcasts of this account, on all its devices. You can follow it again with the address of its feed.'**
  String podcastUnfollowAsk(String title);

  /// No description provided for @stationRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove this station?'**
  String get stationRemoveTitle;

  /// No description provided for @stationRemoveAsk.
  ///
  /// In en, this message translates to:
  /// **'\"{name}\" is taken out of the favourites of this account.'**
  String stationRemoveAsk(String name);

  /// No description provided for @podcastUnfollow.
  ///
  /// In en, this message translates to:
  /// **'Stop following'**
  String get podcastUnfollow;

  /// No description provided for @noEpisodesYet.
  ///
  /// In en, this message translates to:
  /// **'No episodes yet.'**
  String get noEpisodesYet;

  /// No description provided for @episodeAt.
  ///
  /// In en, this message translates to:
  /// **'stopped at {time}'**
  String episodeAt(String time);

  /// No description provided for @stationAdd.
  ///
  /// In en, this message translates to:
  /// **'Add a station'**
  String get stationAdd;

  /// No description provided for @stationName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get stationName;

  /// No description provided for @stationAddress.
  ///
  /// In en, this message translates to:
  /// **'Address of the stream'**
  String get stationAddress;

  /// No description provided for @stationSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get stationSave;

  /// No description provided for @stationAdded.
  ///
  /// In en, this message translates to:
  /// **'“{name}” added'**
  String stationAdded(String name);

  /// No description provided for @stationInvalid.
  ///
  /// In en, this message translates to:
  /// **'A station needs a name and the address of its stream (http or https).'**
  String get stationInvalid;

  /// No description provided for @noStationsYet.
  ///
  /// In en, this message translates to:
  /// **'No stations yet. Tap + to add one.'**
  String get noStationsYet;

  /// No description provided for @stationRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove from the favourites'**
  String get stationRemove;

  /// No description provided for @radioLive.
  ///
  /// In en, this message translates to:
  /// **'Live'**
  String get radioLive;

  /// No description provided for @cdImportButton.
  ///
  /// In en, this message translates to:
  /// **'Import CD'**
  String get cdImportButton;

  /// No description provided for @cdImportTooltip.
  ///
  /// In en, this message translates to:
  /// **'Import the CD in the drive into the library'**
  String get cdImportTooltip;

  /// No description provided for @cdIdentifying.
  ///
  /// In en, this message translates to:
  /// **'Reading the disc…'**
  String get cdIdentifying;

  /// No description provided for @cdFound.
  ///
  /// In en, this message translates to:
  /// **'This disc was found online. Check the details.'**
  String get cdFound;

  /// No description provided for @cdNotFound.
  ///
  /// In en, this message translates to:
  /// **'This disc was not found online. Please enter the details.'**
  String get cdNotFound;

  /// No description provided for @cdCandidate.
  ///
  /// In en, this message translates to:
  /// **'Match'**
  String get cdCandidate;

  /// No description provided for @cdSearchLabel.
  ///
  /// In en, this message translates to:
  /// **'Not the right match? Search MusicBrainz (title, author)'**
  String get cdSearchLabel;

  /// No description provided for @cdSearchGo.
  ///
  /// In en, this message translates to:
  /// **'Search'**
  String get cdSearchGo;

  /// No description provided for @cdSearchNothing.
  ///
  /// In en, this message translates to:
  /// **'Nothing found.'**
  String get cdSearchNothing;

  /// No description provided for @cdAuthor.
  ///
  /// In en, this message translates to:
  /// **'Author'**
  String get cdAuthor;

  /// No description provided for @cdTitle.
  ///
  /// In en, this message translates to:
  /// **'Title'**
  String get cdTitle;

  /// No description provided for @cdDiscNumber.
  ///
  /// In en, this message translates to:
  /// **'CD number'**
  String get cdDiscNumber;

  /// No description provided for @cdDiscsTotal.
  ///
  /// In en, this message translates to:
  /// **'Number of CDs'**
  String get cdDiscsTotal;

  /// No description provided for @cdKind.
  ///
  /// In en, this message translates to:
  /// **'Type'**
  String get cdKind;

  /// No description provided for @cdKindAudiobook.
  ///
  /// In en, this message translates to:
  /// **'Audiobook'**
  String get cdKindAudiobook;

  /// No description provided for @cdKindMusic.
  ///
  /// In en, this message translates to:
  /// **'Music'**
  String get cdKindMusic;

  /// No description provided for @cdEjectAfter.
  ///
  /// In en, this message translates to:
  /// **'Eject the CD afterwards'**
  String get cdEjectAfter;

  /// No description provided for @cdTracks.
  ///
  /// In en, this message translates to:
  /// **'{count} tracks'**
  String cdTracks(int count);

  /// No description provided for @cdSave.
  ///
  /// In en, this message translates to:
  /// **'Import'**
  String get cdSave;

  /// No description provided for @cdClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get cdClose;

  /// No description provided for @cdStopImport.
  ///
  /// In en, this message translates to:
  /// **'Stop import'**
  String get cdStopImport;

  /// No description provided for @cdReading.
  ///
  /// In en, this message translates to:
  /// **'Reading track {track} of {total}'**
  String cdReading(int track, int total);

  /// No description provided for @cdDone.
  ///
  /// In en, this message translates to:
  /// **'The CD is now in the library.'**
  String get cdDone;

  /// No description provided for @cdEjected.
  ///
  /// In en, this message translates to:
  /// **'The CD was ejected.'**
  String get cdEjected;

  /// No description provided for @cdCancelled.
  ///
  /// In en, this message translates to:
  /// **'The import was cancelled. Nothing was added to the library.'**
  String get cdCancelled;

  /// No description provided for @cdErrNoDisc.
  ///
  /// In en, this message translates to:
  /// **'There is no readable audio CD in the drive.'**
  String get cdErrNoDisc;

  /// No description provided for @cdErrUnsupported.
  ///
  /// In en, this message translates to:
  /// **'The server asks for a format or tag version that this app cannot create yet.'**
  String get cdErrUnsupported;

  /// No description provided for @cdErrRead.
  ///
  /// In en, this message translates to:
  /// **'The disc could not be read reliably. It may be scratched or dirty.'**
  String get cdErrRead;

  /// No description provided for @cdErrDriveGone.
  ///
  /// In en, this message translates to:
  /// **'The drive of the server stopped answering.'**
  String get cdErrDriveGone;

  /// No description provided for @cdErrEncode.
  ///
  /// In en, this message translates to:
  /// **'The server could not convert the audio.'**
  String get cdErrEncode;

  /// No description provided for @cdErrForbidden.
  ///
  /// In en, this message translates to:
  /// **'Your account may not add CDs to the library.'**
  String get cdErrForbidden;

  /// No description provided for @cdErrDetails.
  ///
  /// In en, this message translates to:
  /// **'Details'**
  String get cdErrDetails;

  /// No description provided for @cdErrVerify.
  ///
  /// In en, this message translates to:
  /// **'Two readings of a track never gave the same result. The disc may be scratched or dirty.'**
  String get cdErrVerify;

  /// No description provided for @cdErrCancelled.
  ///
  /// In en, this message translates to:
  /// **'The import was cancelled. Nothing was added to the library.'**
  String get cdErrCancelled;

  /// No description provided for @cdErrIo.
  ///
  /// In en, this message translates to:
  /// **'A file could not be written on the server.'**
  String get cdErrIo;

  /// No description provided for @cdErrNetwork.
  ///
  /// In en, this message translates to:
  /// **'The server does not answer.'**
  String get cdErrNetwork;

  /// No description provided for @cdErrCertificate.
  ///
  /// In en, this message translates to:
  /// **'The server\'s certificate is not the one this device was paired with.'**
  String get cdErrCertificate;

  /// No description provided for @cdErrUnauthorized.
  ///
  /// In en, this message translates to:
  /// **'This device is no longer signed in to the server.'**
  String get cdErrUnauthorized;

  /// No description provided for @cdErrInvalidToc.
  ///
  /// In en, this message translates to:
  /// **'The server could not use the table of contents of this disc.'**
  String get cdErrInvalidToc;

  /// No description provided for @cdErrInvalidQuery.
  ///
  /// In en, this message translates to:
  /// **'The search needs at least two letters.'**
  String get cdErrInvalidQuery;

  /// No description provided for @cdErrNoContact.
  ///
  /// In en, this message translates to:
  /// **'The administrator has not set a contact address for MusicBrainz yet, so discs cannot be looked up. You can enter the details by hand.'**
  String get cdErrNoContact;

  /// No description provided for @cdErrLookupFailed.
  ///
  /// In en, this message translates to:
  /// **'The online lookup of the disc did not work. You can enter the details by hand.'**
  String get cdErrLookupFailed;

  /// No description provided for @cdErrNotFound.
  ///
  /// In en, this message translates to:
  /// **'The server does not know this item.'**
  String get cdErrNotFound;

  /// No description provided for @cdErrInvalidRelease.
  ///
  /// In en, this message translates to:
  /// **'The server did not accept the release.'**
  String get cdErrInvalidRelease;

  /// No description provided for @cdErrTrackNumbers.
  ///
  /// In en, this message translates to:
  /// **'The track numbers are not complete or appear twice.'**
  String get cdErrTrackNumbers;

  /// No description provided for @cdErrInvalidMeta.
  ///
  /// In en, this message translates to:
  /// **'The server did not accept the author, title or numbers.'**
  String get cdErrInvalidMeta;

  /// No description provided for @cdErrUnreadableFile.
  ///
  /// In en, this message translates to:
  /// **'The server could not read one of the files.'**
  String get cdErrUnreadableFile;

  /// No description provided for @cdErrFormatMismatch.
  ///
  /// In en, this message translates to:
  /// **'A file does not have the format that the server\'s settings ask for.'**
  String get cdErrFormatMismatch;

  /// No description provided for @cdErrBadExtension.
  ///
  /// In en, this message translates to:
  /// **'A file has an ending that the server does not accept.'**
  String get cdErrBadExtension;

  /// No description provided for @cdErrUnknownFile.
  ///
  /// In en, this message translates to:
  /// **'The server received a file it did not expect.'**
  String get cdErrUnknownFile;

  /// No description provided for @cdErrMissingFile.
  ///
  /// In en, this message translates to:
  /// **'A file is missing in what was sent to the server.'**
  String get cdErrMissingFile;

  /// No description provided for @cdErrBadCover.
  ///
  /// In en, this message translates to:
  /// **'The cover picture was not accepted.'**
  String get cdErrBadCover;

  /// No description provided for @cdErrBadTags.
  ///
  /// In en, this message translates to:
  /// **'The tags of a file could not be written by the server.'**
  String get cdErrBadTags;

  /// No description provided for @cdErrNoImportFolder.
  ///
  /// In en, this message translates to:
  /// **'The server has no import folder for this kind of media.'**
  String get cdErrNoImportFolder;

  /// No description provided for @cdErrNoSpace.
  ///
  /// In en, this message translates to:
  /// **'There is not enough room on the server.'**
  String get cdErrNoSpace;

  /// No description provided for @cdErrDiscExists.
  ///
  /// In en, this message translates to:
  /// **'This CD is already in the library; the server does not overwrite it.'**
  String get cdErrDiscExists;

  /// No description provided for @cdErrTooLarge.
  ///
  /// In en, this message translates to:
  /// **'The upload is larger than the server accepts.'**
  String get cdErrTooLarge;

  /// No description provided for @cdErrBadRequest.
  ///
  /// In en, this message translates to:
  /// **'The server did not understand the request.'**
  String get cdErrBadRequest;

  /// No description provided for @cdErrInvalidJob.
  ///
  /// In en, this message translates to:
  /// **'The server did not accept the import job.'**
  String get cdErrInvalidJob;

  /// No description provided for @cdErrJobClosed.
  ///
  /// In en, this message translates to:
  /// **'The import job is already finished.'**
  String get cdErrJobClosed;

  /// No description provided for @cdErrImportFailed.
  ///
  /// In en, this message translates to:
  /// **'The server could not add the CD to the library.'**
  String get cdErrImportFailed;

  /// No description provided for @cdErrInterrupted.
  ///
  /// In en, this message translates to:
  /// **'The import was interrupted, for example by a restart of the server.'**
  String get cdErrInterrupted;

  /// No description provided for @cdErrStale.
  ///
  /// In en, this message translates to:
  /// **'The import stopped sending news and was given up.'**
  String get cdErrStale;

  /// No description provided for @cdErrClient.
  ///
  /// In en, this message translates to:
  /// **'The device that started the import gave it up.'**
  String get cdErrClient;

  /// No description provided for @cdErrUnknown.
  ///
  /// In en, this message translates to:
  /// **'The CD import failed ({code}).'**
  String cdErrUnknown(String code);

  /// No description provided for @sectionNew.
  ///
  /// In en, this message translates to:
  /// **'New'**
  String get sectionNew;

  /// No description provided for @badgeNew.
  ///
  /// In en, this message translates to:
  /// **'New'**
  String get badgeNew;

  /// No description provided for @tooltipPlayed.
  ///
  /// In en, this message translates to:
  /// **'Played'**
  String get tooltipPlayed;

  /// No description provided for @playsCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Played once} other{Played {count} times}}'**
  String playsCount(int count);

  /// No description provided for @lastPlayedOn.
  ///
  /// In en, this message translates to:
  /// **'last played on {date}'**
  String lastPlayedOn(DateTime date);

  /// No description provided for @cdStarting.
  ///
  /// In en, this message translates to:
  /// **'Starting…'**
  String get cdStarting;

  /// No description provided for @cdEncoding.
  ///
  /// In en, this message translates to:
  /// **'Converting the audio…'**
  String get cdEncoding;

  /// No description provided for @cdAddingToLibrary.
  ///
  /// In en, this message translates to:
  /// **'Adding the CD to the library…'**
  String get cdAddingToLibrary;

  /// No description provided for @cdImportingTooltip.
  ///
  /// In en, this message translates to:
  /// **'Importing the CD'**
  String get cdImportingTooltip;

  /// No description provided for @cdEndPresent.
  ///
  /// In en, this message translates to:
  /// **'This CD is in the library already (ejected): {album}'**
  String cdEndPresent(String album);

  /// No description provided for @cdEndPresentPlain.
  ///
  /// In en, this message translates to:
  /// **'This CD is in the library already (ejected).'**
  String get cdEndPresentPlain;

  /// No description provided for @cdEndRejected.
  ///
  /// In en, this message translates to:
  /// **'The CD was rejected. Its files were deleted.'**
  String get cdEndRejected;

  /// No description provided for @cdEndCancelled.
  ///
  /// In en, this message translates to:
  /// **'The import was cancelled.'**
  String get cdEndCancelled;

  /// No description provided for @cdWaitsApproval.
  ///
  /// In en, this message translates to:
  /// **'Waits for approval'**
  String get cdWaitsApproval;

  /// No description provided for @cdWaitsRetry.
  ///
  /// In en, this message translates to:
  /// **'Read error: the CD is still in the drive'**
  String get cdWaitsRetry;

  /// No description provided for @cdEject.
  ///
  /// In en, this message translates to:
  /// **'Eject'**
  String get cdEject;

  /// No description provided for @cdEjectTooltip.
  ///
  /// In en, this message translates to:
  /// **'Eject CD'**
  String get cdEjectTooltip;

  /// No description provided for @cdNothingToEject.
  ///
  /// In en, this message translates to:
  /// **'There is nothing to eject.'**
  String get cdNothingToEject;

  /// No description provided for @cdErrNoDrive.
  ///
  /// In en, this message translates to:
  /// **'The server has no CD drive.'**
  String get cdErrNoDrive;

  /// No description provided for @cdErrDriveDenied.
  ///
  /// In en, this message translates to:
  /// **'The server may not use its CD drive. The administrator has to allow it.'**
  String get cdErrDriveDenied;

  /// No description provided for @cdErrTrayOpen.
  ///
  /// In en, this message translates to:
  /// **'The tray of the drive is open. Close it by hand first.'**
  String get cdErrTrayOpen;

  /// No description provided for @cdErrBusy.
  ///
  /// In en, this message translates to:
  /// **'The drive is busy with another CD.'**
  String get cdErrBusy;

  /// No description provided for @cdErrEjectFailed.
  ///
  /// In en, this message translates to:
  /// **'The drive did not open its tray.'**
  String get cdErrEjectFailed;

  /// No description provided for @cdErrInternal.
  ///
  /// In en, this message translates to:
  /// **'The server had an internal problem.'**
  String get cdErrInternal;

  /// No description provided for @cdErrInvalidSelection.
  ///
  /// In en, this message translates to:
  /// **'A chosen track is not on the disc, or is chosen twice.'**
  String get cdErrInvalidSelection;

  /// No description provided for @cdSelectAll.
  ///
  /// In en, this message translates to:
  /// **'Select all tracks'**
  String get cdSelectAll;

  /// No description provided for @cdSelectNone.
  ///
  /// In en, this message translates to:
  /// **'Select no track'**
  String get cdSelectNone;

  /// No description provided for @cdSelectInvert.
  ///
  /// In en, this message translates to:
  /// **'Invert the selection'**
  String get cdSelectInvert;

  /// No description provided for @cdTrackDefault.
  ///
  /// In en, this message translates to:
  /// **'Track {number}'**
  String cdTrackDefault(String number);

  /// No description provided for @cdTracksSummary.
  ///
  /// In en, this message translates to:
  /// **'{selected} of {total} tracks, {duration}'**
  String cdTracksSummary(int selected, int total, String duration);

  /// No description provided for @cdErrEmptySelection.
  ///
  /// In en, this message translates to:
  /// **'No track is chosen.'**
  String get cdErrEmptySelection;

  /// No description provided for @cdErrTaken.
  ///
  /// In en, this message translates to:
  /// **'Tracks {numbers} are already in the library.'**
  String cdErrTaken(String numbers);

  /// No description provided for @cdBarHead.
  ///
  /// In en, this message translates to:
  /// **'{title}, CD {disc}'**
  String cdBarHead(String title, int disc);

  /// No description provided for @cdBarTrack.
  ///
  /// In en, this message translates to:
  /// **'Track {track} of {total}'**
  String cdBarTrack(int track, int total);

  /// No description provided for @cdPhaseReading.
  ///
  /// In en, this message translates to:
  /// **'Reading'**
  String get cdPhaseReading;

  /// No description provided for @cdPhaseChecking.
  ///
  /// In en, this message translates to:
  /// **'Checking'**
  String get cdPhaseChecking;

  /// No description provided for @cdPhaseEncoding.
  ///
  /// In en, this message translates to:
  /// **'Converting'**
  String get cdPhaseEncoding;

  /// No description provided for @cdPhasePlacing.
  ///
  /// In en, this message translates to:
  /// **'Filing'**
  String get cdPhasePlacing;

  /// No description provided for @cdPhaseImporting.
  ///
  /// In en, this message translates to:
  /// **'Importing'**
  String get cdPhaseImporting;

  /// No description provided for @cdPhaseWorking.
  ///
  /// In en, this message translates to:
  /// **'Working'**
  String get cdPhaseWorking;

  /// No description provided for @cdBarDone.
  ///
  /// In en, this message translates to:
  /// **'Done: {count} tracks'**
  String cdBarDone(int count);

  /// No description provided for @cdBarDoneNoCount.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get cdBarDoneNoCount;

  /// No description provided for @cdBarImporting.
  ///
  /// In en, this message translates to:
  /// **'Importing: {line}'**
  String cdBarImporting(String line);

  /// No description provided for @cdErrDiscPresent.
  ///
  /// In en, this message translates to:
  /// **'This CD is in the library already.'**
  String get cdErrDiscPresent;

  /// No description provided for @cdErrNotPresent.
  ///
  /// In en, this message translates to:
  /// **'This CD is not in the library, so there is nothing to repair.'**
  String get cdErrNotPresent;

  /// No description provided for @cdErrNothingToHeal.
  ///
  /// In en, this message translates to:
  /// **'The CD is whole in the library; there is nothing to repair.'**
  String get cdErrNothingToHeal;

  /// No description provided for @cdErrUnreadableTrack.
  ///
  /// In en, this message translates to:
  /// **'Some tracks could not be read from the CD (it may be scratched).'**
  String get cdErrUnreadableTrack;

  /// No description provided for @cdStateUnverified.
  ///
  /// In en, this message translates to:
  /// **'Taken, not confirmed'**
  String get cdStateUnverified;

  /// No description provided for @cdErrUnverifiedTrack.
  ///
  /// In en, this message translates to:
  /// **'A track was not read the same way twice and was taken with its first reading (not confirmed).'**
  String get cdErrUnverifiedTrack;

  /// No description provided for @cdErrResumeMismatch.
  ///
  /// In en, this message translates to:
  /// **'The settings or the choice of tracks changed, so the CD is read again from the start.'**
  String get cdErrResumeMismatch;

  /// No description provided for @cdErrHealNeeded.
  ///
  /// In en, this message translates to:
  /// **'The CD is in the library, but tracks are missing or damaged. Repair it from the import dialog.'**
  String get cdErrHealNeeded;

  /// No description provided for @cdErrResumePending.
  ///
  /// In en, this message translates to:
  /// **'An interrupted reading of this CD is waiting. Continue it from the import dialog.'**
  String get cdErrResumePending;

  /// No description provided for @cdErrAmbiguousMatch.
  ///
  /// In en, this message translates to:
  /// **'Several releases fit this CD. Please choose one.'**
  String get cdErrAmbiguousMatch;

  /// No description provided for @cdErrNoMatch.
  ///
  /// In en, this message translates to:
  /// **'The online database does not know this CD.'**
  String get cdErrNoMatch;

  /// No description provided for @cdErrNoTarget.
  ///
  /// In en, this message translates to:
  /// **'The server has no import folder that it may write to.'**
  String get cdErrNoTarget;

  /// No description provided for @cdErrUdevOff.
  ///
  /// In en, this message translates to:
  /// **'The automatic import needs the detection of drives by system events to be switched on.'**
  String get cdErrUdevOff;

  /// No description provided for @cdErrNotAudio.
  ///
  /// In en, this message translates to:
  /// **'This is not an audio CD.'**
  String get cdErrNotAudio;

  /// No description provided for @cdErrStartFailed.
  ///
  /// In en, this message translates to:
  /// **'The import could not be started.'**
  String get cdErrStartFailed;

  /// No description provided for @cdErrLocked.
  ///
  /// In en, this message translates to:
  /// **'The CD was added, but the import of the library is locked. It shows up when the lock is lifted.'**
  String get cdErrLocked;

  /// No description provided for @cdContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get cdContinue;

  /// No description provided for @cdResumeNote.
  ///
  /// In en, this message translates to:
  /// **'This CD was read to {done} of {total} tracks already (interrupted {when}).'**
  String cdResumeNote(int done, int total, String when);

  /// No description provided for @cdRereadFromStart.
  ///
  /// In en, this message translates to:
  /// **'Read again from the start'**
  String get cdRereadFromStart;

  /// No description provided for @cdResumeLost.
  ///
  /// In en, this message translates to:
  /// **'An earlier reading of this CD cannot be continued, because the settings changed. The CD is read again.'**
  String get cdResumeLost;

  /// No description provided for @cdResumed.
  ///
  /// In en, this message translates to:
  /// **'Continuing an earlier reading.'**
  String get cdResumed;

  /// No description provided for @cdImportAnyway.
  ///
  /// In en, this message translates to:
  /// **'Import anyway'**
  String get cdImportAnyway;

  /// No description provided for @cdHeal.
  ///
  /// In en, this message translates to:
  /// **'Check and repair'**
  String get cdHeal;

  /// No description provided for @cdForceTitle.
  ///
  /// In en, this message translates to:
  /// **'Import the CD again?'**
  String get cdForceTitle;

  /// No description provided for @cdForceBody.
  ///
  /// In en, this message translates to:
  /// **'This CD is in the library already. This adds it a second time, as a new copy. Only do that if you really want two copies.'**
  String get cdForceBody;

  /// No description provided for @cdForceConfirm.
  ///
  /// In en, this message translates to:
  /// **'Import as a new copy'**
  String get cdForceConfirm;

  /// No description provided for @cdPresentTitle.
  ///
  /// In en, this message translates to:
  /// **'This CD is in the library already'**
  String get cdPresentTitle;

  /// No description provided for @cdPresentBookDisc.
  ///
  /// In en, this message translates to:
  /// **'{book}, CD {disc}'**
  String cdPresentBookDisc(String book, int disc);

  /// No description provided for @cdPresentBookAuthorDisc.
  ///
  /// In en, this message translates to:
  /// **'{book} by {author}, CD {disc}'**
  String cdPresentBookAuthorDisc(String book, String author, int disc);

  /// No description provided for @cdPresentComplete.
  ///
  /// In en, this message translates to:
  /// **'The CD is whole in the library ({inLibrary} of {onDisc} tracks).'**
  String cdPresentComplete(int inLibrary, int onDisc);

  /// No description provided for @cdPresentCount.
  ///
  /// In en, this message translates to:
  /// **'{inLibrary} of {onDisc} tracks are in the library.'**
  String cdPresentCount(int inLibrary, int onDisc);

  /// No description provided for @cdMissing.
  ///
  /// In en, this message translates to:
  /// **'Missing'**
  String get cdMissing;

  /// No description provided for @cdDamaged.
  ///
  /// In en, this message translates to:
  /// **'Damaged'**
  String get cdDamaged;

  /// No description provided for @cdDamagedEmpty.
  ///
  /// In en, this message translates to:
  /// **'the file is empty'**
  String get cdDamagedEmpty;

  /// No description provided for @cdDamagedUnreadable.
  ///
  /// In en, this message translates to:
  /// **'the file cannot be read'**
  String get cdDamagedUnreadable;

  /// No description provided for @cdDamagedLength.
  ///
  /// In en, this message translates to:
  /// **'the file is {have} long, the CD has {want}'**
  String cdDamagedLength(String have, String want);

  /// No description provided for @cdDamagedOther.
  ///
  /// In en, this message translates to:
  /// **'the file does not match the CD'**
  String get cdDamagedOther;

  /// No description provided for @cdHealHint.
  ///
  /// In en, this message translates to:
  /// **'Only the missing and damaged tracks are read from the CD. A damaged file is kept in the quarantine folder.'**
  String get cdHealHint;

  /// No description provided for @cdStateWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting'**
  String get cdStateWaiting;

  /// No description provided for @cdStateRead.
  ///
  /// In en, this message translates to:
  /// **'Read, waits for the check'**
  String get cdStateRead;

  /// No description provided for @cdSpeed.
  ///
  /// In en, this message translates to:
  /// **'Drive reads at {x}x'**
  String cdSpeed(String x);

  /// No description provided for @cdStateRereading.
  ///
  /// In en, this message translates to:
  /// **'Reading again'**
  String get cdStateRereading;

  /// No description provided for @cdStateRereadingPercent.
  ///
  /// In en, this message translates to:
  /// **'Reading again {percent} %'**
  String cdStateRereadingPercent(int percent);

  /// No description provided for @cdStateReading.
  ///
  /// In en, this message translates to:
  /// **'Reading'**
  String get cdStateReading;

  /// No description provided for @cdStateReadingPercent.
  ///
  /// In en, this message translates to:
  /// **'Reading {percent} %'**
  String cdStateReadingPercent(int percent);

  /// No description provided for @cdStateVerifying.
  ///
  /// In en, this message translates to:
  /// **'Checking'**
  String get cdStateVerifying;

  /// No description provided for @cdStateEncoding.
  ///
  /// In en, this message translates to:
  /// **'Converting'**
  String get cdStateEncoding;

  /// No description provided for @cdStateDone.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get cdStateDone;

  /// No description provided for @cdStateFailed.
  ///
  /// In en, this message translates to:
  /// **'Error'**
  String get cdStateFailed;

  /// No description provided for @cdBuffer.
  ///
  /// In en, this message translates to:
  /// **'Buffer {seconds} s of {max} s'**
  String cdBuffer(int seconds, int max);

  /// No description provided for @filterPlaylists.
  ///
  /// In en, this message translates to:
  /// **'Playlists'**
  String get filterPlaylists;

  /// No description provided for @playlistNew.
  ///
  /// In en, this message translates to:
  /// **'New playlist'**
  String get playlistNew;

  /// No description provided for @playlistNewItem.
  ///
  /// In en, this message translates to:
  /// **'New playlist…'**
  String get playlistNewItem;

  /// No description provided for @playlistName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get playlistName;

  /// No description provided for @playlistSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get playlistSave;

  /// No description provided for @playlistsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No playlists yet. Tap + to make one.'**
  String get playlistsEmpty;

  /// No description provided for @playlistMeta.
  ///
  /// In en, this message translates to:
  /// **'{count} titles · {duration}'**
  String playlistMeta(int count, String duration);

  /// No description provided for @playlistAddTo.
  ///
  /// In en, this message translates to:
  /// **'Add to playlist'**
  String get playlistAddTo;

  /// No description provided for @playlistAddedTo.
  ///
  /// In en, this message translates to:
  /// **'Added to “{name}”.'**
  String playlistAddedTo(String name);

  /// No description provided for @playlistChoose.
  ///
  /// In en, this message translates to:
  /// **'Add to which playlist?'**
  String get playlistChoose;

  /// No description provided for @playlistPlay.
  ///
  /// In en, this message translates to:
  /// **'Play'**
  String get playlistPlay;

  /// No description provided for @playlistRename.
  ///
  /// In en, this message translates to:
  /// **'Rename'**
  String get playlistRename;

  /// No description provided for @playlistRenameTitle.
  ///
  /// In en, this message translates to:
  /// **'Rename the playlist'**
  String get playlistRenameTitle;

  /// No description provided for @playlistDelete.
  ///
  /// In en, this message translates to:
  /// **'Delete playlist'**
  String get playlistDelete;

  /// No description provided for @playlistDeleteTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete this playlist?'**
  String get playlistDeleteTitle;

  /// No description provided for @playlistDeleteText.
  ///
  /// In en, this message translates to:
  /// **'“{name}” is deleted from your profile. The titles stay in the library.'**
  String playlistDeleteText(String name);

  /// No description provided for @playlistRemoveEntry.
  ///
  /// In en, this message translates to:
  /// **'Remove from the playlist'**
  String get playlistRemoveEntry;

  /// No description provided for @playlistUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Not available'**
  String get playlistUnavailable;

  /// No description provided for @playlistEmpty.
  ///
  /// In en, this message translates to:
  /// **'This playlist is empty. Use “Add to playlist” on a title or a book.'**
  String get playlistEmpty;

  /// No description provided for @playlistDownload.
  ///
  /// In en, this message translates to:
  /// **'Download the playlist'**
  String get playlistDownload;

  /// No description provided for @plErrNameTaken.
  ///
  /// In en, this message translates to:
  /// **'You have a playlist with this name already.'**
  String get plErrNameTaken;

  /// No description provided for @plErrInvalidName.
  ///
  /// In en, this message translates to:
  /// **'The name must not be empty and at most 100 characters.'**
  String get plErrInvalidName;

  /// No description provided for @plErrUnknownPart.
  ///
  /// In en, this message translates to:
  /// **'A title is not there any more, or you may not use it.'**
  String get plErrUnknownPart;

  /// No description provided for @plErrUnknownBook.
  ///
  /// In en, this message translates to:
  /// **'The book is not there any more, or you may not use it.'**
  String get plErrUnknownBook;

  /// No description provided for @plErrInvalidOrder.
  ///
  /// In en, this message translates to:
  /// **'The order could not be used. The list is shown as it is on the server.'**
  String get plErrInvalidOrder;

  /// No description provided for @plErrInvalidPosition.
  ///
  /// In en, this message translates to:
  /// **'That place in the playlist does not exist.'**
  String get plErrInvalidPosition;

  /// No description provided for @plErrInvalidRequest.
  ///
  /// In en, this message translates to:
  /// **'The request was not valid.'**
  String get plErrInvalidRequest;

  /// No description provided for @plErrPlaylistFull.
  ///
  /// In en, this message translates to:
  /// **'The playlist is full (at most 5000 titles).'**
  String get plErrPlaylistFull;

  /// No description provided for @plErrTooManyPlaylists.
  ///
  /// In en, this message translates to:
  /// **'You have the most playlists you can have (100).'**
  String get plErrTooManyPlaylists;

  /// No description provided for @plErrNotFound.
  ///
  /// In en, this message translates to:
  /// **'The playlist is not there any more.'**
  String get plErrNotFound;

  /// No description provided for @plErrNetwork.
  ///
  /// In en, this message translates to:
  /// **'The server does not answer.'**
  String get plErrNetwork;

  /// No description provided for @plErrUnknown.
  ///
  /// In en, this message translates to:
  /// **'The playlist could not be changed ({code}).'**
  String plErrUnknown(String code);

  /// No description provided for @hideMenuTooltip.
  ///
  /// In en, this message translates to:
  /// **'Hide or show'**
  String get hideMenuTooltip;

  /// No description provided for @hideBook.
  ///
  /// In en, this message translates to:
  /// **'Hide this book'**
  String get hideBook;

  /// No description provided for @showBook.
  ///
  /// In en, this message translates to:
  /// **'Show this book again'**
  String get showBook;

  /// No description provided for @hideAuthorAudiobooks.
  ///
  /// In en, this message translates to:
  /// **'Hide all audiobooks by {name}'**
  String hideAuthorAudiobooks(String name);

  /// No description provided for @showAuthorAudiobooks.
  ///
  /// In en, this message translates to:
  /// **'Show the audiobooks by {name} again'**
  String showAuthorAudiobooks(String name);

  /// No description provided for @hideAuthorMusic.
  ///
  /// In en, this message translates to:
  /// **'Hide all music by {name}'**
  String hideAuthorMusic(String name);

  /// No description provided for @showAuthorMusic.
  ///
  /// In en, this message translates to:
  /// **'Show the music by {name} again'**
  String showAuthorMusic(String name);

  /// No description provided for @hideAuthorAll.
  ///
  /// In en, this message translates to:
  /// **'Hide everything by {name}'**
  String hideAuthorAll(String name);

  /// No description provided for @showAuthorAll.
  ///
  /// In en, this message translates to:
  /// **'Show everything by {name} again'**
  String showAuthorAll(String name);

  /// No description provided for @hiddenMark.
  ///
  /// In en, this message translates to:
  /// **'Hidden'**
  String get hiddenMark;

  /// No description provided for @hidingFailed.
  ///
  /// In en, this message translates to:
  /// **'The change could not be saved.'**
  String get hidingFailed;

  /// No description provided for @settingsMyLibrary.
  ///
  /// In en, this message translates to:
  /// **'My library'**
  String get settingsMyLibrary;

  /// No description provided for @settingsShowHidden.
  ///
  /// In en, this message translates to:
  /// **'Show what I hid'**
  String get settingsShowHidden;

  /// No description provided for @settingsShowHiddenHint.
  ///
  /// In en, this message translates to:
  /// **'What you hide leaves your lists, only yours. With this switch it is listed again, dimmed, so that you can show it again.'**
  String get settingsShowHiddenHint;

  /// No description provided for @settingsHiddenTitle.
  ///
  /// In en, this message translates to:
  /// **'Hidden'**
  String get settingsHiddenTitle;

  /// No description provided for @settingsHiddenNone.
  ///
  /// In en, this message translates to:
  /// **'You have not hidden anything. Long-press a book, or open it and use the menu at the top.'**
  String get settingsHiddenNone;

  /// No description provided for @settingsHiddenAuthorAudiobooks.
  ///
  /// In en, this message translates to:
  /// **'Audiobooks by {name}'**
  String settingsHiddenAuthorAudiobooks(String name);

  /// No description provided for @settingsHiddenAuthorMusic.
  ///
  /// In en, this message translates to:
  /// **'Music by {name}'**
  String settingsHiddenAuthorMusic(String name);

  /// No description provided for @settingsHiddenAuthorAll.
  ///
  /// In en, this message translates to:
  /// **'Everything by {name}'**
  String settingsHiddenAuthorAll(String name);

  /// No description provided for @settingsShowAgain.
  ///
  /// In en, this message translates to:
  /// **'Show again'**
  String get settingsShowAgain;

  /// No description provided for @musicOf.
  ///
  /// In en, this message translates to:
  /// **'Music of {name}'**
  String musicOf(String name);

  /// No description provided for @musicOfNobody.
  ///
  /// In en, this message translates to:
  /// **'Other music'**
  String get musicOfNobody;

  /// No description provided for @releasedLabel.
  ///
  /// In en, this message translates to:
  /// **'Released: {year}'**
  String releasedLabel(int year);

  /// No description provided for @ownerLabel.
  ///
  /// In en, this message translates to:
  /// **'Owner: {name}'**
  String ownerLabel(String name);

  /// No description provided for @audiobooksOf.
  ///
  /// In en, this message translates to:
  /// **'Audiobooks of {name}'**
  String audiobooksOf(String name);

  /// No description provided for @audiobooksOfNobody.
  ///
  /// In en, this message translates to:
  /// **'Other audiobooks'**
  String get audiobooksOfNobody;

  /// No description provided for @groupExpand.
  ///
  /// In en, this message translates to:
  /// **'Expand'**
  String get groupExpand;

  /// No description provided for @groupCollapse.
  ///
  /// In en, this message translates to:
  /// **'Collapse'**
  String get groupCollapse;

  /// No description provided for @settingsWebsite.
  ///
  /// In en, this message translates to:
  /// **'Website of the server'**
  String get settingsWebsite;

  /// No description provided for @settingsWebsiteHint.
  ///
  /// In en, this message translates to:
  /// **'Opens the start page of the server in the browser, with the way to the administration.'**
  String get settingsWebsiteHint;

  /// No description provided for @settingsWebsiteFailed.
  ///
  /// In en, this message translates to:
  /// **'The browser could not be opened.'**
  String get settingsWebsiteFailed;

  /// No description provided for @shareTooltip.
  ///
  /// In en, this message translates to:
  /// **'Share'**
  String get shareTooltip;

  /// No description provided for @shareText.
  ///
  /// In en, this message translates to:
  /// **'{title} by {author}'**
  String shareText(String title, String author);

  /// No description provided for @labelsTitle.
  ///
  /// In en, this message translates to:
  /// **'Labels'**
  String get labelsTitle;

  /// No description provided for @labelsEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit labels'**
  String get labelsEdit;

  /// No description provided for @labelsNone.
  ///
  /// In en, this message translates to:
  /// **'No labels yet.'**
  String get labelsNone;

  /// No description provided for @trackLabelsEdit.
  ///
  /// In en, this message translates to:
  /// **'Labels of this title'**
  String get trackLabelsEdit;

  /// No description provided for @labelsAdd.
  ///
  /// In en, this message translates to:
  /// **'Add'**
  String get labelsAdd;

  /// No description provided for @labelsNew.
  ///
  /// In en, this message translates to:
  /// **'New label'**
  String get labelsNew;

  /// No description provided for @labelsSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get labelsSave;

  /// No description provided for @labelsFailed.
  ///
  /// In en, this message translates to:
  /// **'The labels could not be saved.'**
  String get labelsFailed;

  /// No description provided for @labelsBad.
  ///
  /// In en, this message translates to:
  /// **'That is not a usable label: 1 to 40 characters, no comma, semicolon or bar.'**
  String get labelsBad;

  /// No description provided for @labelsFilterClear.
  ///
  /// In en, this message translates to:
  /// **'All labels'**
  String get labelsFilterClear;

  /// No description provided for @labelsRename.
  ///
  /// In en, this message translates to:
  /// **'Rename label'**
  String get labelsRename;

  /// No description provided for @labelsDelete.
  ///
  /// In en, this message translates to:
  /// **'Delete label'**
  String get labelsDelete;

  /// No description provided for @labelsNoneInLibrary.
  ///
  /// In en, this message translates to:
  /// **'No labels in the library yet. The owner of a book sets them on its page.'**
  String get labelsNoneInLibrary;

  /// No description provided for @labelsDeleteConfirm.
  ///
  /// In en, this message translates to:
  /// **'Take \"{name}\" off {count} books?'**
  String labelsDeleteConfirm(String name, int count);

  /// No description provided for @offlineBannerManual.
  ///
  /// In en, this message translates to:
  /// **'Offline mode · only downloaded titles are shown'**
  String get offlineBannerManual;

  /// No description provided for @offlineBannerUnreachable.
  ///
  /// In en, this message translates to:
  /// **'Server not reachable · only downloaded titles are shown'**
  String get offlineBannerUnreachable;

  /// No description provided for @offlinePending.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 change waits for the server} other{{count} changes wait for the server}}'**
  String offlinePending(int count);

  /// No description provided for @offlineRetry.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get offlineRetry;

  /// No description provided for @offlineNeedsNetwork.
  ///
  /// In en, this message translates to:
  /// **'This page needs the server. It is back as soon as you are online.'**
  String get offlineNeedsNetwork;

  /// No description provided for @offlineSettingTitle.
  ///
  /// In en, this message translates to:
  /// **'Offline mode'**
  String get offlineSettingTitle;

  /// No description provided for @offlineSettingHint.
  ///
  /// In en, this message translates to:
  /// **'Shows only the titles downloaded on this device and does not ask the server. What you change meanwhile (labels, hidden albums, where you stopped listening) waits and is sent when you switch it off.'**
  String get offlineSettingHint;

  /// No description provided for @offlineSyncNow.
  ///
  /// In en, this message translates to:
  /// **'Send changes now'**
  String get offlineSyncNow;

  /// No description provided for @offlineSyncing.
  ///
  /// In en, this message translates to:
  /// **'Sending changes …'**
  String get offlineSyncing;

  /// No description provided for @offlineNothingWaits.
  ///
  /// In en, this message translates to:
  /// **'No changes are waiting.'**
  String get offlineNothingWaits;

  /// No description provided for @offlineRejectedTitle.
  ///
  /// In en, this message translates to:
  /// **'Not accepted by the server'**
  String get offlineRejectedTitle;

  /// No description provided for @offlineRejectedLabels.
  ///
  /// In en, this message translates to:
  /// **'Labels of “{title}” ({reason})'**
  String offlineRejectedLabels(String title, String reason);

  /// No description provided for @offlineRejectedHide.
  ///
  /// In en, this message translates to:
  /// **'Hiding “{title}” ({reason})'**
  String offlineRejectedHide(String title, String reason);

  /// No description provided for @offlineReasonForbidden.
  ///
  /// In en, this message translates to:
  /// **'not allowed'**
  String get offlineReasonForbidden;

  /// No description provided for @offlineReasonGone.
  ///
  /// In en, this message translates to:
  /// **'no longer there'**
  String get offlineReasonGone;

  /// No description provided for @offlineReasonOther.
  ///
  /// In en, this message translates to:
  /// **'refused'**
  String get offlineReasonOther;

  /// No description provided for @offlineDismiss.
  ///
  /// In en, this message translates to:
  /// **'OK'**
  String get offlineDismiss;

  /// No description provided for @profileTitle.
  ///
  /// In en, this message translates to:
  /// **'My profile'**
  String get profileTitle;

  /// No description provided for @profileSettingsHint.
  ///
  /// In en, this message translates to:
  /// **'Name, picture, language and PIN'**
  String get profileSettingsHint;

  /// No description provided for @profilePicture.
  ///
  /// In en, this message translates to:
  /// **'Picture'**
  String get profilePicture;

  /// No description provided for @profileTakePhoto.
  ///
  /// In en, this message translates to:
  /// **'Take a photo'**
  String get profileTakePhoto;

  /// No description provided for @profileChoosePhoto.
  ///
  /// In en, this message translates to:
  /// **'Choose a picture'**
  String get profileChoosePhoto;

  /// No description provided for @profileRemovePhotoTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove the picture?'**
  String get profileRemovePhotoTitle;

  /// No description provided for @profileRemovePhotoAsk.
  ///
  /// In en, this message translates to:
  /// **'Your picture is deleted on the server and on all your devices.'**
  String get profileRemovePhotoAsk;

  /// No description provided for @profileRemovePhoto.
  ///
  /// In en, this message translates to:
  /// **'Remove picture'**
  String get profileRemovePhoto;

  /// No description provided for @profileNickname.
  ///
  /// In en, this message translates to:
  /// **'Nickname'**
  String get profileNickname;

  /// No description provided for @profileFirstName.
  ///
  /// In en, this message translates to:
  /// **'First name'**
  String get profileFirstName;

  /// No description provided for @profileLastName.
  ///
  /// In en, this message translates to:
  /// **'Last name'**
  String get profileLastName;

  /// No description provided for @profileLanguage.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get profileLanguage;

  /// No description provided for @profileLanguageAuto.
  ///
  /// In en, this message translates to:
  /// **'Follow the device'**
  String get profileLanguageAuto;

  /// No description provided for @profileRole.
  ///
  /// In en, this message translates to:
  /// **'Role'**
  String get profileRole;

  /// No description provided for @roleAdmin.
  ///
  /// In en, this message translates to:
  /// **'Administrator'**
  String get roleAdmin;

  /// No description provided for @roleMember.
  ///
  /// In en, this message translates to:
  /// **'Member'**
  String get roleMember;

  /// No description provided for @roleChild.
  ///
  /// In en, this message translates to:
  /// **'Child'**
  String get roleChild;

  /// No description provided for @roleGuest.
  ///
  /// In en, this message translates to:
  /// **'Guest'**
  String get roleGuest;

  /// No description provided for @profileDevices.
  ///
  /// In en, this message translates to:
  /// **'My devices'**
  String get profileDevices;

  /// No description provided for @profileDeviceThis.
  ///
  /// In en, this message translates to:
  /// **'this device'**
  String get profileDeviceThis;

  /// No description provided for @profileNeverSeen.
  ///
  /// In en, this message translates to:
  /// **'never seen'**
  String get profileNeverSeen;

  /// No description provided for @profileSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get profileSave;

  /// No description provided for @profileSaved.
  ///
  /// In en, this message translates to:
  /// **'Saved.'**
  String get profileSaved;

  /// No description provided for @profilePinTitle.
  ///
  /// In en, this message translates to:
  /// **'Change PIN or password'**
  String get profilePinTitle;

  /// No description provided for @profilePinCurrent.
  ///
  /// In en, this message translates to:
  /// **'Current PIN or password'**
  String get profilePinCurrent;

  /// No description provided for @profilePinNew.
  ///
  /// In en, this message translates to:
  /// **'New PIN or password'**
  String get profilePinNew;

  /// No description provided for @profilePinRepeat.
  ///
  /// In en, this message translates to:
  /// **'Repeat the new one'**
  String get profilePinRepeat;

  /// No description provided for @profilePinChanged.
  ///
  /// In en, this message translates to:
  /// **'The PIN has been changed.'**
  String get profilePinChanged;

  /// No description provided for @profileErrOffline.
  ///
  /// In en, this message translates to:
  /// **'This needs the server. Try again when you are connected.'**
  String get profileErrOffline;

  /// No description provided for @profileErrExists.
  ///
  /// In en, this message translates to:
  /// **'This nickname is taken.'**
  String get profileErrExists;

  /// No description provided for @profileErrName.
  ///
  /// In en, this message translates to:
  /// **'This name cannot be used.'**
  String get profileErrName;

  /// No description provided for @profileErrPinWrong.
  ///
  /// In en, this message translates to:
  /// **'The current PIN is not right.'**
  String get profileErrPinWrong;

  /// No description provided for @profileErrPinInvalid.
  ///
  /// In en, this message translates to:
  /// **'The new PIN or password cannot be used.'**
  String get profileErrPinInvalid;

  /// No description provided for @profileErrPinMismatch.
  ///
  /// In en, this message translates to:
  /// **'The two new entries differ.'**
  String get profileErrPinMismatch;

  /// No description provided for @profileErrLocked.
  ///
  /// In en, this message translates to:
  /// **'Too many wrong tries. Try again later.'**
  String get profileErrLocked;

  /// No description provided for @profilePinWait.
  ///
  /// In en, this message translates to:
  /// **'The PIN can be changed again in {time}'**
  String profilePinWait(String time);

  /// No description provided for @profileErrAvatarType.
  ///
  /// In en, this message translates to:
  /// **'This file is not a picture.'**
  String get profileErrAvatarType;

  /// No description provided for @profileErrAvatarSize.
  ///
  /// In en, this message translates to:
  /// **'The picture is too large.'**
  String get profileErrAvatarSize;

  /// No description provided for @profileErrCameraDenied.
  ///
  /// In en, this message translates to:
  /// **'The camera or the photos may not be used. Allow it in the settings of the system.'**
  String get profileErrCameraDenied;

  /// No description provided for @profileErrCameraNone.
  ///
  /// In en, this message translates to:
  /// **'There is no camera to use here.'**
  String get profileErrCameraNone;

  /// No description provided for @profileErrGeneric.
  ///
  /// In en, this message translates to:
  /// **'That did not work.'**
  String get profileErrGeneric;

  /// No description provided for @profileAvatarTooltip.
  ///
  /// In en, this message translates to:
  /// **'Open your profile'**
  String get profileAvatarTooltip;

  /// No description provided for @profileLastSeen.
  ///
  /// In en, this message translates to:
  /// **'last seen {date}'**
  String profileLastSeen(String date);

  /// No description provided for @profileDeviceCount.
  ///
  /// In en, this message translates to:
  /// **'Devices: {used} of {max}'**
  String profileDeviceCount(int used, int max);

  /// No description provided for @profileDeviceCountFree.
  ///
  /// In en, this message translates to:
  /// **'Devices: {used}'**
  String profileDeviceCountFree(int used);

  /// No description provided for @episodeDownload.
  ///
  /// In en, this message translates to:
  /// **'Download episode'**
  String get episodeDownload;

  /// No description provided for @episodeDownloadCancel.
  ///
  /// In en, this message translates to:
  /// **'Stop the download'**
  String get episodeDownloadCancel;

  /// No description provided for @episodeDownloaded.
  ///
  /// In en, this message translates to:
  /// **'On this device'**
  String get episodeDownloaded;

  /// No description provided for @episodeRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove the download?'**
  String get episodeRemoveTitle;

  /// No description provided for @episodeRemoveText.
  ///
  /// In en, this message translates to:
  /// **'The episode is deleted from this device. It stays in the podcast and can be downloaded again.'**
  String get episodeRemoveText;

  /// No description provided for @episodeProgress.
  ///
  /// In en, this message translates to:
  /// **'{done} of {total}'**
  String episodeProgress(String done, String total);

  /// No description provided for @episodeErrorScheme.
  ///
  /// In en, this message translates to:
  /// **'This episode has no http or https address and cannot be downloaded.'**
  String get episodeErrorScheme;

  /// No description provided for @episodeErrorHttp.
  ///
  /// In en, this message translates to:
  /// **'The publisher refused the download.'**
  String get episodeErrorHttp;

  /// No description provided for @episodeErrorTooLarge.
  ///
  /// In en, this message translates to:
  /// **'The episode is larger than the limit of {limit} in the settings.'**
  String episodeErrorTooLarge(String limit);

  /// No description provided for @episodeErrorNetwork.
  ///
  /// In en, this message translates to:
  /// **'The download failed: the publisher cannot be reached.'**
  String get episodeErrorNetwork;

  /// No description provided for @episodeErrorIo.
  ///
  /// In en, this message translates to:
  /// **'The file could not be saved on this device.'**
  String get episodeErrorIo;

  /// No description provided for @podcastDownloadNewest.
  ///
  /// In en, this message translates to:
  /// **'Download the newest {count} unheard episodes'**
  String podcastDownloadNewest(int count);

  /// No description provided for @podcastDownloadNewestNone.
  ///
  /// In en, this message translates to:
  /// **'Nothing new to download.'**
  String get podcastDownloadNewestNone;

  /// No description provided for @podcastDownloadsTitle.
  ///
  /// In en, this message translates to:
  /// **'Podcast downloads'**
  String get podcastDownloadsTitle;

  /// No description provided for @podcastDownloadsUsed.
  ///
  /// In en, this message translates to:
  /// **'{size} in {count} episodes'**
  String podcastDownloadsUsed(String size, int count);

  /// No description provided for @podcastDownloadsNone.
  ///
  /// In en, this message translates to:
  /// **'No podcast episodes on this device.'**
  String get podcastDownloadsNone;

  /// No description provided for @podcastDownloadsDeleteAll.
  ///
  /// In en, this message translates to:
  /// **'Delete all'**
  String get podcastDownloadsDeleteAll;

  /// No description provided for @podcastDownloadsDeleteAllTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete all podcast downloads?'**
  String get podcastDownloadsDeleteAllTitle;

  /// No description provided for @podcastDownloadsDeleteAllText.
  ///
  /// In en, this message translates to:
  /// **'All downloaded episodes are deleted from this device. The podcasts keep them.'**
  String get podcastDownloadsDeleteAllText;

  /// No description provided for @podcastDeleteAfter.
  ///
  /// In en, this message translates to:
  /// **'Delete episodes after listening'**
  String get podcastDeleteAfter;

  /// No description provided for @podcastDeleteAfterHint.
  ///
  /// In en, this message translates to:
  /// **'A heard episode is deleted from this device, and so is one that the podcast does not list any more. Off: nothing is deleted unless you do it.'**
  String get podcastDeleteAfterHint;

  /// No description provided for @podcastMaxSize.
  ///
  /// In en, this message translates to:
  /// **'Largest download'**
  String get podcastMaxSize;

  /// No description provided for @podcastDownloadAll.
  ///
  /// In en, this message translates to:
  /// **'Download the whole collection'**
  String get podcastDownloadAll;

  /// No description provided for @podcastDownloadAllTitle.
  ///
  /// In en, this message translates to:
  /// **'Download the whole collection?'**
  String get podcastDownloadAllTitle;

  /// No description provided for @podcastDownloadAllText.
  ///
  /// In en, this message translates to:
  /// **'{count} episodes, about {size}. They are loaded one after the other and take that much space on this device. You can stop at any time.'**
  String podcastDownloadAllText(int count, String size);

  /// No description provided for @podcastDownloadAllLimited.
  ///
  /// In en, this message translates to:
  /// **'The collection has {count} episodes, about {size}. That is more than your limit of {limit}, so only the newest {fitCount} (about {fitSize}) will be loaded. You can raise the limit under Settings > Podcast downloads.'**
  String podcastDownloadAllLimited(
    int count,
    String size,
    String limit,
    int fitCount,
    String fitSize,
  );

  /// No description provided for @podcastDownloadAllBlocked.
  ///
  /// In en, this message translates to:
  /// **'Even the newest episode is larger than your limit of {limit}. You can raise the limit under Settings > Podcast downloads.'**
  String podcastDownloadAllBlocked(String limit);

  /// No description provided for @podcastDownloadAllStart.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get podcastDownloadAllStart;

  /// No description provided for @podcastDownloadAllRunning.
  ///
  /// In en, this message translates to:
  /// **'Downloading {done} of {total} ...'**
  String podcastDownloadAllRunning(int done, int total);

  /// No description provided for @podcastDownloadAllStop.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get podcastDownloadAllStop;

  /// No description provided for @podcastDownloadAllDone.
  ///
  /// In en, this message translates to:
  /// **'{count} episodes loaded.'**
  String podcastDownloadAllDone(int count);

  /// No description provided for @podcastBulkLimit.
  ///
  /// In en, this message translates to:
  /// **'Largest collection download'**
  String get podcastBulkLimit;

  /// No description provided for @podcastBulkLimitHint.
  ///
  /// In en, this message translates to:
  /// **'The most that \"Download the whole collection\" loads in one go.'**
  String get podcastBulkLimitHint;

  /// No description provided for @podcastBulkWifiOnly.
  ///
  /// In en, this message translates to:
  /// **'Collection downloads only on Wi-Fi'**
  String get podcastBulkWifiOnly;

  /// No description provided for @podcastBulkWifiOnlyHint.
  ///
  /// In en, this message translates to:
  /// **'\"Download the whole collection\" does not load on mobile data. A cable or Wi-Fi is fine.'**
  String get podcastBulkWifiOnlyHint;

  /// No description provided for @podcastDownloadAllMobile.
  ///
  /// In en, this message translates to:
  /// **'You are on mobile data. The whole collection is only loaded on Wi-Fi (you can change that under Settings > Podcast downloads).'**
  String get podcastDownloadAllMobile;

  /// No description provided for @podcastDownloadAllSpace.
  ///
  /// In en, this message translates to:
  /// **'Only {free} are free on this device, and 1 GB should stay free. Of the {count} episodes (about {size}) the newest {fitCount} (about {fitSize}) will be loaded.'**
  String podcastDownloadAllSpace(
    String free,
    int count,
    String size,
    int fitCount,
    String fitSize,
  );

  /// No description provided for @podcastDownloadAllNoSpace.
  ///
  /// In en, this message translates to:
  /// **'There is not enough room on this device: only {free} are free, and 1 GB should stay free. Free some space and try again.'**
  String podcastDownloadAllNoSpace(String free);

  /// No description provided for @podcastMegabytes.
  ///
  /// In en, this message translates to:
  /// **'{mb} MB'**
  String podcastMegabytes(int mb);

  /// No description provided for @offlinePodcastsNone.
  ///
  /// In en, this message translates to:
  /// **'No episodes are on this device yet. Download episodes while you are online to listen to them here.'**
  String get offlinePodcastsNone;

  /// No description provided for @barcodeCheckTooltip.
  ///
  /// In en, this message translates to:
  /// **'Check a barcode'**
  String get barcodeCheckTooltip;

  /// No description provided for @barcodeTitle.
  ///
  /// In en, this message translates to:
  /// **'Check a barcode'**
  String get barcodeTitle;

  /// No description provided for @barcodeFieldLabel.
  ///
  /// In en, this message translates to:
  /// **'Barcode'**
  String get barcodeFieldLabel;

  /// No description provided for @barcodeCheck.
  ///
  /// In en, this message translates to:
  /// **'Check'**
  String get barcodeCheck;

  /// No description provided for @barcodeInLibrary.
  ///
  /// In en, this message translates to:
  /// **'In the library:'**
  String get barcodeInLibrary;

  /// No description provided for @barcodeNotInLibrary.
  ///
  /// In en, this message translates to:
  /// **'Not in the library: {code}'**
  String barcodeNotInLibrary(String code);

  /// No description provided for @barcodeInvalid.
  ///
  /// In en, this message translates to:
  /// **'This is not a valid barcode. Check the digits.'**
  String get barcodeInvalid;

  /// No description provided for @barcodeOfflineNotFound.
  ///
  /// In en, this message translates to:
  /// **'The library on this device does not have it. The server was not asked, because you are offline.'**
  String get barcodeOfflineNotFound;

  /// No description provided for @barcodeLookupFailed.
  ///
  /// In en, this message translates to:
  /// **'The server could not be asked. Try again later.'**
  String get barcodeLookupFailed;

  /// No description provided for @barcodeOpen.
  ///
  /// In en, this message translates to:
  /// **'Open'**
  String get barcodeOpen;

  /// No description provided for @barcodeScanHint.
  ///
  /// In en, this message translates to:
  /// **'Hold the barcode of the CD in front of the camera'**
  String get barcodeScanHint;

  /// No description provided for @barcodeTypeHint.
  ///
  /// In en, this message translates to:
  /// **'Or type the digits below the barcode:'**
  String get barcodeTypeHint;

  /// No description provided for @inboxTitle.
  ///
  /// In en, this message translates to:
  /// **'CD inbox'**
  String get inboxTitle;

  /// No description provided for @inboxNotificationTitle.
  ///
  /// In en, this message translates to:
  /// **'CDs'**
  String get inboxNotificationTitle;

  /// No description provided for @inboxCardWaiting.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 CD waits for approval} other{{count} CDs wait for approval}}'**
  String inboxCardWaiting(int count);

  /// No description provided for @inboxCardUnread.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 unread} other{{count} unread}}'**
  String inboxCardUnread(int count);

  /// No description provided for @inboxOpen.
  ///
  /// In en, this message translates to:
  /// **'Open the inbox'**
  String get inboxOpen;

  /// No description provided for @inboxEmpty.
  ///
  /// In en, this message translates to:
  /// **'No messages about your CDs.'**
  String get inboxEmpty;

  /// No description provided for @inboxMarkAllRead.
  ///
  /// In en, this message translates to:
  /// **'Mark all as read'**
  String get inboxMarkAllRead;

  /// No description provided for @inboxImport.
  ///
  /// In en, this message translates to:
  /// **'Import'**
  String get inboxImport;

  /// No description provided for @inboxReject.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get inboxReject;

  /// No description provided for @cdGiveUp.
  ///
  /// In en, this message translates to:
  /// **'Give up'**
  String get cdGiveUp;

  /// No description provided for @cdGiveUpTitle.
  ///
  /// In en, this message translates to:
  /// **'Give up on this CD?'**
  String get cdGiveUpTitle;

  /// No description provided for @cdGiveUpText.
  ///
  /// In en, this message translates to:
  /// **'The import ends. What was read is kept for a while, so reading the CD again later is quicker.'**
  String get cdGiveUpText;

  /// No description provided for @cdSpeakAwaiting.
  ///
  /// In en, this message translates to:
  /// **'The CD {title} is done and waits for your approval.'**
  String cdSpeakAwaiting(String title);

  /// No description provided for @cdSpeakAwaitingNoTitle.
  ///
  /// In en, this message translates to:
  /// **'A CD is done and waits for your approval.'**
  String get cdSpeakAwaitingNoTitle;

  /// No description provided for @cdSpeakImported.
  ///
  /// In en, this message translates to:
  /// **'The CD {title} was imported.'**
  String cdSpeakImported(String title);

  /// No description provided for @cdSpeakImportedNoTitle.
  ///
  /// In en, this message translates to:
  /// **'A CD was imported.'**
  String get cdSpeakImportedNoTitle;

  /// No description provided for @cdSpeakReadError.
  ///
  /// In en, this message translates to:
  /// **'The CD {title} has a read error.'**
  String cdSpeakReadError(String title);

  /// No description provided for @cdSpeakReadErrorNoTitle.
  ///
  /// In en, this message translates to:
  /// **'A CD has a read error.'**
  String get cdSpeakReadErrorNoTitle;

  /// No description provided for @cdSpeakPresent.
  ///
  /// In en, this message translates to:
  /// **'The CD {title} is in the library already.'**
  String cdSpeakPresent(String title);

  /// No description provided for @cdSpeakPresentNoTitle.
  ///
  /// In en, this message translates to:
  /// **'A CD is in the library already.'**
  String get cdSpeakPresentNoTitle;

  /// No description provided for @cdAnnounceTest.
  ///
  /// In en, this message translates to:
  /// **'This is how the announcements about CDs sound.'**
  String get cdAnnounceTest;

  /// No description provided for @cdAnnounceTitle.
  ///
  /// In en, this message translates to:
  /// **'CD announcements'**
  String get cdAnnounceTitle;

  /// No description provided for @cdAnnounceHint.
  ///
  /// In en, this message translates to:
  /// **'While music plays, a voice says when a CD is done or needs you.'**
  String get cdAnnounceHint;

  /// No description provided for @cdAnnounceTestButton.
  ///
  /// In en, this message translates to:
  /// **'Test'**
  String get cdAnnounceTestButton;

  /// No description provided for @cdAnnounceNoSpdSay.
  ///
  /// In en, this message translates to:
  /// **'No voice: the program spd-say (speech-dispatcher) is missing.'**
  String get cdAnnounceNoSpdSay;

  /// No description provided for @cdAnnounceNoVoice.
  ///
  /// In en, this message translates to:
  /// **'This device has no voice for announcements yet.'**
  String get cdAnnounceNoVoice;

  /// No description provided for @inboxRetry.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get inboxRetry;

  /// No description provided for @inboxOpenAlbum.
  ///
  /// In en, this message translates to:
  /// **'Open album'**
  String get inboxOpenAlbum;

  /// No description provided for @inboxRejectTitle.
  ///
  /// In en, this message translates to:
  /// **'Reject this CD?'**
  String get inboxRejectTitle;

  /// No description provided for @inboxRejectText.
  ///
  /// In en, this message translates to:
  /// **'Its files are deleted.'**
  String get inboxRejectText;

  /// No description provided for @inboxWaitsForApproval.
  ///
  /// In en, this message translates to:
  /// **'Waits for approval'**
  String get inboxWaitsForApproval;

  /// No description provided for @inboxWorking.
  ///
  /// In en, this message translates to:
  /// **'One moment…'**
  String get inboxWorking;

  /// No description provided for @inboxResolvedApproved.
  ///
  /// In en, this message translates to:
  /// **'Imported by {by}'**
  String inboxResolvedApproved(String by);

  /// No description provided for @inboxResolvedRejected.
  ///
  /// In en, this message translates to:
  /// **'Rejected by {by}'**
  String inboxResolvedRejected(String by);

  /// No description provided for @inboxResolvedApprovedYou.
  ///
  /// In en, this message translates to:
  /// **'Imported'**
  String get inboxResolvedApprovedYou;

  /// No description provided for @inboxResolvedRejectedYou.
  ///
  /// In en, this message translates to:
  /// **'Rejected'**
  String get inboxResolvedRejectedYou;

  /// No description provided for @inboxResolvedExpired.
  ///
  /// In en, this message translates to:
  /// **'Expired'**
  String get inboxResolvedExpired;

  /// No description provided for @inboxAlreadyDecided.
  ///
  /// In en, this message translates to:
  /// **'Already decided by {by}'**
  String inboxAlreadyDecided(String by);

  /// No description provided for @inboxNeedsServer.
  ///
  /// In en, this message translates to:
  /// **'The inbox needs the server. It is back as soon as you are online.'**
  String get inboxNeedsServer;

  /// No description provided for @inboxLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'The inbox could not be loaded.'**
  String get inboxLoadFailed;

  /// No description provided for @inboxErrNetwork.
  ///
  /// In en, this message translates to:
  /// **'The server could not be reached.'**
  String get inboxErrNetwork;

  /// No description provided for @inboxErrForbidden.
  ///
  /// In en, this message translates to:
  /// **'You may not decide CDs.'**
  String get inboxErrForbidden;

  /// No description provided for @inboxErrBusy.
  ///
  /// In en, this message translates to:
  /// **'The drive is busy.'**
  String get inboxErrBusy;

  /// No description provided for @inboxErrNoDisc.
  ///
  /// In en, this message translates to:
  /// **'There is no CD in the drive.'**
  String get inboxErrNoDisc;

  /// No description provided for @inboxErrDrive.
  ///
  /// In en, this message translates to:
  /// **'The drive cannot be used now.'**
  String get inboxErrDrive;

  /// No description provided for @inboxErrGeneric.
  ///
  /// In en, this message translates to:
  /// **'That did not work.'**
  String get inboxErrGeneric;

  /// No description provided for @inboxUnknown.
  ///
  /// In en, this message translates to:
  /// **'News from the server that this version of the app does not know yet.'**
  String get inboxUnknown;

  /// No description provided for @inboxThisCd.
  ///
  /// In en, this message translates to:
  /// **'a CD'**
  String get inboxThisCd;

  /// No description provided for @inboxDiscOf.
  ///
  /// In en, this message translates to:
  /// **'CD {disc} of {total}'**
  String inboxDiscOf(int disc, int total);

  /// No description provided for @inboxDiscNumber.
  ///
  /// In en, this message translates to:
  /// **'CD {disc}'**
  String inboxDiscNumber(int disc);

  /// No description provided for @inboxCdAwaitingApproval.
  ///
  /// In en, this message translates to:
  /// **'New CD from {by}: {album}. Import it?'**
  String inboxCdAwaitingApproval(String by, String album);

  /// No description provided for @inboxCdAwaitingApprovalAuto.
  ///
  /// In en, this message translates to:
  /// **'New CD put in at the server: {album}. Import it?'**
  String inboxCdAwaitingApprovalAuto(String album);

  /// No description provided for @inboxCdWaiting.
  ///
  /// In en, this message translates to:
  /// **'{album} waits for approval.'**
  String inboxCdWaiting(String album);

  /// No description provided for @inboxCdImported.
  ///
  /// In en, this message translates to:
  /// **'{album} is in the library ({tracks, plural, =1{1 track} other{{tracks} tracks}}).'**
  String inboxCdImported(String album, int tracks);

  /// No description provided for @inboxCdImportedNoCount.
  ///
  /// In en, this message translates to:
  /// **'{album} is in the library.'**
  String inboxCdImportedNoCount(String album);

  /// No description provided for @inboxCdPresent.
  ///
  /// In en, this message translates to:
  /// **'{album} is in the library already. The CD was not read.'**
  String inboxCdPresent(String album);

  /// No description provided for @inboxCdDecidedApproved.
  ///
  /// In en, this message translates to:
  /// **'{by} imported the CD {album}.'**
  String inboxCdDecidedApproved(String by, String album);

  /// No description provided for @inboxCdDecidedRejected.
  ///
  /// In en, this message translates to:
  /// **'{by} rejected the CD {album}.'**
  String inboxCdDecidedRejected(String by, String album);

  /// No description provided for @inboxCdExpired.
  ///
  /// In en, this message translates to:
  /// **'The CD {album} was not decided in time. Its files were deleted.'**
  String inboxCdExpired(String album);

  /// No description provided for @inboxCdReadError.
  ///
  /// In en, this message translates to:
  /// **'{album}: could not read track {tracks}.'**
  String inboxCdReadError(String album, String tracks);

  /// No description provided for @inboxCdReadErrorNoTracks.
  ///
  /// In en, this message translates to:
  /// **'{album}: some tracks could not be read.'**
  String inboxCdReadErrorNoTracks(String album);

  /// No description provided for @inboxCdNoMatch.
  ///
  /// In en, this message translates to:
  /// **'The CD in the drive is not known to MusicBrainz. Choose it in the web panel.'**
  String get inboxCdNoMatch;

  /// No description provided for @inboxCdAmbiguous.
  ///
  /// In en, this message translates to:
  /// **'The CD fits {candidates, plural, =1{1 release} other{{candidates} releases}}. Choose the right one in the web panel.'**
  String inboxCdAmbiguous(int candidates);

  /// No description provided for @inboxCdAmbiguousNoCount.
  ///
  /// In en, this message translates to:
  /// **'The CD fits several releases. Choose the right one in the web panel.'**
  String get inboxCdAmbiguousNoCount;

  /// No description provided for @inboxCdHealNeeded.
  ///
  /// In en, this message translates to:
  /// **'{album} is in the library only in part. It must be repaired before the CD is read.'**
  String inboxCdHealNeeded(String album);

  /// No description provided for @inboxCdNoSpace.
  ///
  /// In en, this message translates to:
  /// **'There is not enough free space to read {album}.'**
  String inboxCdNoSpace(String album);

  /// No description provided for @inboxCdNoTarget.
  ///
  /// In en, this message translates to:
  /// **'{album} was not read: no folder for imported CDs is set up.'**
  String inboxCdNoTarget(String album);

  /// No description provided for @inboxAlbumNotFound.
  ///
  /// In en, this message translates to:
  /// **'The album is not in the library of this device yet. Refresh the library.'**
  String get inboxAlbumNotFound;

  /// No description provided for @cdErrInvalidMove.
  ///
  /// In en, this message translates to:
  /// **'This CD is not in the state the step needs. The list is read again.'**
  String get cdErrInvalidMove;

  /// No description provided for @cdErrDecided.
  ///
  /// In en, this message translates to:
  /// **'Somebody else was faster and decided this CD.'**
  String get cdErrDecided;

  /// No description provided for @cdErrInvalidPaging.
  ///
  /// In en, this message translates to:
  /// **'The page of the inbox asked for does not exist.'**
  String get cdErrInvalidPaging;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['de', 'en'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'de':
      return AppLocalizationsDe();
    case 'en':
      return AppLocalizationsEn();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
