// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// Generated with the texts of the slideshow of the browser (server/src/admin/messages.rs, pictures.*); the choices are those of
// server/src/show_project.rs.
import '../l10n/app_localizations.dart';

/// The choices of a slideshow and their options, in the order of the page `/pictures`.
const kShowChoices = <String, List<String>>{
  'order': ['date', 'random'],
  'seconds': ['3', '5', '8', '15', '30'],
  'transition': ['fade', 'zoom', 'cut', 'random'],
  'videos': ['on', 'off'],
  'info': ['both', 'date', 'none'],
};

/// The default of each choice.
const kShowDefaults = <String, String>{'order': 'date', 'seconds': '8', 'transition': 'fade', 'videos': 'on', 'info': 'both'};

/// The name of a choice.
String showChoiceLabel(AppLocalizations l, String key) => switch (key) {
      'order' => l.showSetOrder,
      'seconds' => l.showSetSeconds,
      'transition' => l.showSetTransition,
      'videos' => l.showSetVideos,
      'info' => l.showSetInfo,
      _ => key,
    };

/// The name of an option.
String showOptionLabel(AppLocalizations l, String key, String option) => switch ((key, option)) {
      ('order', 'date') => l.showOptOrderDate,
      ('order', 'random') => l.showOptOrderRandom,
      ('seconds', '3') => l.showOptSeconds3,
      ('seconds', '5') => l.showOptSeconds5,
      ('seconds', '8') => l.showOptSeconds8,
      ('seconds', '15') => l.showOptSeconds15,
      ('seconds', '30') => l.showOptSeconds30,
      ('transition', 'fade') => l.showOptTransitionFade,
      ('transition', 'zoom') => l.showOptTransitionZoom,
      ('transition', 'cut') => l.showOptTransitionCut,
      ('transition', 'random') => l.showOptTransitionRandom,
      ('videos', 'on') => l.showOptVideosOn,
      ('videos', 'off') => l.showOptVideosOff,
      ('info', 'both') => l.showOptInfoBoth,
      ('info', 'date') => l.showOptInfoDate,
      ('info', 'none') => l.showOptInfoNone,
      _ => option,
    };

/// What an option does.
String showOptionExplain(AppLocalizations l, String key, String option) => switch ((key, option)) {
      ('order', 'date') => l.showExplainOrderDate,
      ('order', 'random') => l.showExplainOrderRandom,
      ('seconds', '3') => l.showExplainSeconds3,
      ('seconds', '5') => l.showExplainSeconds5,
      ('seconds', '8') => l.showExplainSeconds8,
      ('seconds', '15') => l.showExplainSeconds15,
      ('seconds', '30') => l.showExplainSeconds30,
      ('transition', 'fade') => l.showExplainTransitionFade,
      ('transition', 'zoom') => l.showExplainTransitionZoom,
      ('transition', 'cut') => l.showExplainTransitionCut,
      ('transition', 'random') => l.showExplainTransitionRandom,
      ('videos', 'on') => l.showExplainVideosOn,
      ('videos', 'off') => l.showExplainVideosOff,
      ('info', 'both') => l.showExplainInfoBoth,
      ('info', 'date') => l.showExplainInfoDate,
      ('info', 'none') => l.showExplainInfoNone,
      _ => '',
    };
