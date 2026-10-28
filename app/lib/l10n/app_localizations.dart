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
  /// **'Psalm'**
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
  /// **'Read QR code from a picture'**
  String get qrButton;

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

  /// No description provided for @settingsDisconnectHint.
  ///
  /// In en, this message translates to:
  /// **'Forgets the pairing with the server on this device. To connect again you need a new pairing code.'**
  String get settingsDisconnectHint;

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
  /// **'Psalm plays audiobooks, music and more from your own David server, for the whole family.'**
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
  /// **'Psalm is only the player. It needs a David server, which holds your audiobooks, music and the family\'s profiles. Without one, Psalm has nothing to play.'**
  String get connectNeedsDavid;

  /// No description provided for @aboutDavidTitle.
  ///
  /// In en, this message translates to:
  /// **'Psalm needs a David server'**
  String get aboutDavidTitle;

  /// No description provided for @aboutDavidText.
  ///
  /// In en, this message translates to:
  /// **'Psalm does not come with any content. It plays what a David server offers: audiobooks, music and more from your own library, with a profile for each family member. David is free software that you run yourself, on a home server or a small computer. Whoever runs it creates a pairing code on its admin page; you enter that code here once. Without a David server you can only try the demo data.'**
  String get aboutDavidText;

  /// No description provided for @aboutDavidSteps.
  ///
  /// In en, this message translates to:
  /// **'1. Run a David server.  2. Open its admin page and choose Pair a device.  3. Scan or paste the code in Psalm.'**
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
  /// **'Psalm talks only to the David server you paired it with, over a connection that checks the server\'s certificate. It has no analytics and no advertising. The pairing and the downloaded files stay on this device.'**
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
