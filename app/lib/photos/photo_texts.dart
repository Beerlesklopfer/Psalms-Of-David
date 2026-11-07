// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// Generated with the texts of the photo backup; the choices and codes are those of server/src/photo_settings.rs.
import '../l10n/app_localizations.dart';

/// The name of a choice of the profile.
String photoChoiceLabel(AppLocalizations l, String key) => switch (key) {
      'enabled' => l.photoSetEnabled,
      'mode' => l.photoSetMode,
      'network' => l.photoSetNetwork,
      'charging' => l.photoSetCharging,
      'videos' => l.photoSetVideos,
      'deletes' => l.photoSetDeletes,
      'parent_view' => l.photoSetParentView,
      _ => key,
    };

/// The name of an option of a choice.
String photoOptionLabel(AppLocalizations l, String key, String option) => switch ((key, option)) {
      ('enabled', 'off') => l.photoOptEnabledOff,
      ('enabled', 'on') => l.photoOptEnabledOn,
      ('mode', 'uni') => l.photoOptModeUni,
      ('mode', 'bi') => l.photoOptModeBi,
      ('network', 'wifi') => l.photoOptNetworkWifi,
      ('network', 'any') => l.photoOptNetworkAny,
      ('charging', 'charging') => l.photoOptChargingCharging,
      ('charging', 'always') => l.photoOptChargingAlways,
      ('videos', 'on') => l.photoOptVideosOn,
      ('videos', 'off') => l.photoOptVideosOff,
      ('deletes', 'never') => l.photoOptDeletesNever,
      ('deletes', 'trash') => l.photoOptDeletesTrash,
      ('parent_view', 'off') => l.photoOptParentViewOff,
      ('parent_view', 'on') => l.photoOptParentViewOn,
      _ => option,
    };

/// What an option does.
String photoOptionExplain(AppLocalizations l, String key, String option) => switch ((key, option)) {
      ('enabled', 'off') => l.photoExplainEnabledOff,
      ('enabled', 'on') => l.photoExplainEnabledOn,
      ('mode', 'uni') => l.photoExplainModeUni,
      ('mode', 'bi') => l.photoExplainModeBi,
      ('network', 'wifi') => l.photoExplainNetworkWifi,
      ('network', 'any') => l.photoExplainNetworkAny,
      ('charging', 'charging') => l.photoExplainChargingCharging,
      ('charging', 'always') => l.photoExplainChargingAlways,
      ('videos', 'on') => l.photoExplainVideosOn,
      ('videos', 'off') => l.photoExplainVideosOff,
      ('deletes', 'never') => l.photoExplainDeletesNever,
      ('deletes', 'trash') => l.photoExplainDeletesTrash,
      ('parent_view', 'off') => l.photoExplainParentViewOff,
      ('parent_view', 'on') => l.photoExplainParentViewOn,
      _ => '',
    };

/// Why an option cannot be chosen (a code of the server).
String photoWhy(AppLocalizations l, String why) => switch (why) {
      'server_off' => l.photoWhyServerOff,
      'no_children' => l.photoWhyNoChildren,
      'parents_only' => l.photoWhyParentsOnly,
      'no_guests' => l.photoWhyNoGuests,
      'no_bisync' => l.photoWhyNoBisync,
      'no_videos' => l.photoWhyNoVideos,
      'children_only' => l.photoWhyChildrenOnly,
      'no_parent_view' => l.photoWhyNoParentView,
      'not_an_option' => l.photoWhyNotAnOption,
      'no_folder' => l.photoWhyNoFolder,
      _ => l.photoRefused,
    };

/// What went wrong with a picture (a code of the server or of the device).
String photoErrorText(AppLocalizations l, String code) => switch (code) {
      'too_large' => l.photoErrTooLarge,
      'wrong_type' => l.photoErrWrongType,
      'quota_full' => l.photoErrQuotaFull,
      'disk_full' => l.photoErrDiskFull,
      'unreadable' => l.photoErrUnreadable,
      'hash_mismatch' => l.photoErrHashMismatch,
      'network' => l.photoErrNetwork,
      'backup_off' => l.photoErrBackupOff,
      'no_folder' => l.photoErrNoFolder,
      _ => l.photoErrOther,
    };
