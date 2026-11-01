// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:dbus/dbus.dart';

import '../player_controller.dart';

/// The name under which the desktop finds the player (MPRIS): the keyboard's media keys, the sound menu of
/// the desktop and tools like `playerctl` talk to it. It follows the application id.
const mprisName = 'org.mpris.MediaPlayer2.com.the-abraham-box.psalm';
const _rootInterface = 'org.mpris.MediaPlayer2';
const _playerInterface = 'org.mpris.MediaPlayer2.Player';
const _desktopEntry = 'com.the-abraham-box.psalm';

/// What the player tells the desktop (`org.mpris.MediaPlayer2.Player`), from the state of the controller.
Map<String, DBusValue> playerProperties(PlayerController p) {
  final book = p.book;
  final track = p.track;
  final status = book == null ? 'Stopped' : (p.playing ? 'Playing' : 'Paused');
  final metadata = <String, DBusValue>{
    'mpris:trackid': DBusObjectPath(
      '/com/the_abraham_box/psalm/track/${book == null ? 'none' : '${book.id.hashCode.abs()}_${p.trackIndex}'}',
    ),
    if (track != null) 'mpris:length': DBusInt64(track.length.inMicroseconds),
    if (track != null) 'xesam:title': DBusString(p.nowPlaying ?? track.title),
    if (book != null) 'xesam:album': DBusString(book.title),
    if (book != null) 'xesam:artist': DBusArray.string([book.author]),
    if (book?.coverUrl != null) 'mpris:artUrl': DBusString(book!.coverUrl!),
  };
  final hasItem = book != null;
  return {
    'PlaybackStatus': DBusString(status),
    'Metadata': DBusDict.stringVariant(metadata),
    'Rate': DBusDouble(p.speed),
    'MinimumRate': DBusDouble(0.5),
    'MaximumRate': DBusDouble(3.0),
    'Volume': DBusDouble(1.0),
    'Position': DBusInt64(p.position.inMicroseconds),
    'CanGoNext': DBusBoolean(hasItem && p.trackIndex < book.tracks.length - 1),
    'CanGoPrevious': DBusBoolean(hasItem),
    'CanPlay': DBusBoolean(hasItem),
    'CanPause': DBusBoolean(hasItem),
    'CanSeek': DBusBoolean(hasItem),
    'CanControl': DBusBoolean(true),
  };
}

Map<String, DBusValue> rootProperties() => {
  'CanQuit': DBusBoolean(false),
  'CanRaise': DBusBoolean(false),
  'HasTrackList': DBusBoolean(false),
  'Identity': DBusString('Psalms'),
  'DesktopEntry': DBusString(_desktopEntry),
  'SupportedUriSchemes': DBusArray.string(const []),
  'SupportedMimeTypes': DBusArray.string(const []),
};

/// What a media key or the sound menu asks for: the method of the Player interface and its arguments.
/// Returns false for a method that is not known.
bool performPlayerMethod(PlayerController p, String name, List<DBusValue> args) {
  switch (name) {
    case 'PlayPause':
      p.toggle();
    case 'Play':
      if (!p.playing) p.toggle();
    case 'Pause' || 'Stop':
      if (p.playing) p.toggle();
    case 'Next':
      p.next();
    case 'Previous':
      p.previous();
    case 'Seek':
      // Relative, in microseconds, also across tracks.
      if (args.isNotEmpty) p.skip(Duration(microseconds: args.first.asInt64()));
    case 'SetPosition':
      // The position inside the current track; the track id says which one it was meant for.
      if (args.length == 2 &&
          args[0].asObjectPath() ==
              playerProperties(p)['Metadata']!.asStringVariantDict()['mpris:trackid']!.asObjectPath()) {
        p.seek(Duration(microseconds: args[1].asInt64()));
      }
    case 'OpenUri':
      break;
    default:
      return false;
  }
  return true;
}

/// The object on the session bus. It follows the controller: a change of the state is announced to the desktop.
class MprisService extends DBusObject {
  MprisService._(this._player, this._client) : super(DBusObjectPath('/org/mpris/MediaPlayer2'));

  final PlayerController _player;
  final DBusClient _client;
  Map<String, DBusValue> _last = {};

  /// Takes the name on the session bus. Without a session bus (no desktop, a container) it returns null and the app
  /// runs without media keys.
  static Future<MprisService?> start(PlayerController player) async {
    try {
      final client = DBusClient.session();
      final service = MprisService._(player, client);
      await client.registerObject(service);
      final reply = await client.requestName(mprisName);
      if (reply == DBusRequestNameReply.exists || reply == DBusRequestNameReply.inQueue) {
        await client.close();
        return null;
      }
      service._last = playerProperties(player);
      player.addListener(service._changed);
      return service;
    } on Object {
      return null;
    }
  }

  void _changed() {
    final now = playerProperties(_player);
    // Position is not announced (it changes all the time; the desktop asks for it).
    final changed = <String, DBusValue>{
      for (final e in now.entries)
        if (e.key != 'Position' && _last[e.key] != e.value) e.key: e.value,
    };
    _last = now;
    if (changed.isNotEmpty) unawaited(emitPropertiesChanged(_playerInterface, changedProperties: changed));
  }

  Future<void> stop() async {
    _player.removeListener(_changed);
    await _client.close();
  }

  @override
  List<DBusIntrospectInterface> introspect() => [
    DBusIntrospectInterface(
      _rootInterface,
      methods: [DBusIntrospectMethod('Raise'), DBusIntrospectMethod('Quit')],
      properties: [
        for (final e in rootProperties().entries)
          DBusIntrospectProperty(e.key, e.value.signature, access: DBusPropertyAccess.read),
      ],
    ),
    DBusIntrospectInterface(
      _playerInterface,
      methods: [
        for (final m in ['Next', 'Previous', 'Pause', 'PlayPause', 'Stop', 'Play']) DBusIntrospectMethod(m),
        DBusIntrospectMethod(
          'Seek',
          args: [DBusIntrospectArgument(DBusSignature('x'), DBusArgumentDirection.in_, name: 'Offset')],
        ),
        DBusIntrospectMethod(
          'SetPosition',
          args: [
            DBusIntrospectArgument(DBusSignature('o'), DBusArgumentDirection.in_, name: 'TrackId'),
            DBusIntrospectArgument(DBusSignature('x'), DBusArgumentDirection.in_, name: 'Position'),
          ],
        ),
      ],
      properties: [
        for (final e in playerProperties(_player).entries)
          DBusIntrospectProperty(
            e.key,
            e.value.signature,
            access: e.key == 'Rate' || e.key == 'Volume' ? DBusPropertyAccess.readwrite : DBusPropertyAccess.read,
          ),
      ],
    ),
  ];

  @override
  Future<DBusMethodResponse> handleMethodCall(DBusMethodCall methodCall) async {
    if (methodCall.interface == _rootInterface) {
      return methodCall.name == 'Raise' || methodCall.name == 'Quit'
          ? DBusMethodSuccessResponse()
          : DBusMethodErrorResponse.unknownMethod();
    }
    if (methodCall.interface != _playerInterface) return DBusMethodErrorResponse.unknownInterface();
    return performPlayerMethod(_player, methodCall.name, methodCall.values)
        ? DBusMethodSuccessResponse()
        : DBusMethodErrorResponse.unknownMethod();
  }

  @override
  Future<DBusMethodResponse> getProperty(String interface, String name) async {
    final value = (interface == _rootInterface
        ? rootProperties()
        : interface == _playerInterface
        ? playerProperties(_player)
        : null)?[name];
    return value == null ? DBusMethodErrorResponse.unknownProperty() : DBusGetPropertyResponse(value);
  }

  @override
  Future<DBusMethodResponse> getAllProperties(String interface) async => DBusGetAllPropertiesResponse(
    interface == _rootInterface
        ? rootProperties()
        : interface == _playerInterface
        ? playerProperties(_player)
        : {},
  );

  @override
  Future<DBusMethodResponse> setProperty(String interface, String name, DBusValue value) async {
    if (interface == _playerInterface && name == 'Rate') {
      // The speed is the controller's own choice; the desktop's wish is not taken.
      return DBusMethodSuccessResponse();
    }
    return interface == _playerInterface && name == 'Volume'
        ? DBusMethodSuccessResponse()
        : DBusMethodErrorResponse.unknownProperty();
  }
}
