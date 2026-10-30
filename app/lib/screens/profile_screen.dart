// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../profile.dart';
import '../widgets.dart';
import 'avatar_picker.dart';
import 'confirm_dialog.dart';

/// The picture of a person in a circle: the photo, or the first letter of the name.
class AvatarCircle extends StatelessWidget {
  const AvatarCircle({super.key, required this.bytes, required this.name, this.size = 40});

  final Uint8List? bytes;
  final String name;
  final double size;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final initial = name.trim().isEmpty ? '' : name.trim().characters.first.toUpperCase();
    return SizedBox.square(
      dimension: size,
      child: ClipOval(
        child: bytes != null
            ? Image.memory(bytes!, key: const ValueKey('avatar-image'), fit: BoxFit.cover, gaplessPlayback: true)
            : ColoredBox(
                color: cs.primaryContainer,
                child: Center(
                  child: initial.isEmpty
                      ? Icon(Icons.person, size: size * .6, color: cs.onPrimaryContainer)
                      : Text(initial, style: TextStyle(fontSize: size * .45, color: cs.onPrimaryContainer, fontWeight: FontWeight.w600)),
                ),
              ),
      ),
    );
  }
}

/// The picture of the own profile, drawn from what the app knows (also without the server).
class OwnAvatar extends StatelessWidget {
  const OwnAvatar({super.key, this.size = 40});

  final double size;

  @override
  Widget build(BuildContext context) {
    final p = ProfileScope.maybeOf(context);
    return AvatarCircle(bytes: p?.avatarBytes, name: p?.profile?.name ?? '', size: size);
  }
}

/// Opens the profile page.
void openProfile(BuildContext context) {
  Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const ProfileScreen()));
}

String _errorText(AppLocalizations l, String code) => switch (code) {
      'offline' || 'network' => l.profileErrOffline,
      'exists' => l.profileErrExists,
      'invalid_name' => l.profileErrName,
      'wrong_pin' => l.profileErrPinWrong,
      'invalid_pin' => l.profileErrPinInvalid,
      'locked' => l.profileErrLocked,
      'avatar_type' => l.profileErrAvatarType,
      'avatar_size' => l.profileErrAvatarSize,
      'denied' => l.profileErrCameraDenied,
      'unavailable' => l.profileErrCameraNone,
      _ => l.profileErrGeneric,
    };

String _roleName(AppLocalizations l, String role) => switch (role) {
      'admin' => l.roleAdmin,
      'member' => l.roleMember,
      'child' => l.roleChild,
      'guest' => l.roleGuest,
      _ => role,
    };

/// What a person changes about themself: the picture (camera, photos), the names, the language and the PIN. Role and limits only the administrator
/// changes. A change needs the server; without it the page shows what the app kept and says so.
class ProfileScreen extends StatefulWidget {
  const ProfileScreen({super.key});

  @override
  State<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends State<ProfileScreen> {
  final _nick = TextEditingController();
  final _first = TextEditingController();
  final _last = TextEditingController();
  final _current = TextEditingController();
  final _new = TextEditingController();
  final _repeat = TextEditingController();
  String _language = '';
  bool _filled = false;
  String? _pinError;

  ProfileController? get _c => ProfileScope.maybeOf(context);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _c?.load());
    // The buttons follow what is typed.
    for (final t in [_nick, _current, _new]) {
      t.addListener(_changed);
    }
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    for (final t in [_nick, _first, _last, _current, _new, _repeat]) {
      t.dispose();
    }
    super.dispose();
  }

  /// Fills the fields once, from the first profile that is known.
  void _fill(Profile p) {
    if (_filled) return;
    _filled = true;
    _nick.text = p.name;
    _first.text = p.firstName ?? '';
    _last.text = p.lastName ?? '';
    _language = p.language ?? '';
  }

  void _say(String text) => ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text(text)));

  Future<void> _pick(AvatarSource source) async {
    final c = _c;
    if (c == null) return;
    final l = AppLocalizations.of(context);
    if (!c.available) return _say(l.profileErrOffline);
    final picker = c.picker ?? const PlatformAvatarPicker();
    try {
      final raw = await picker.pick(source);
      if (raw == null || !mounted) return;
      final jpeg = await c.prepare(raw);
      if (!mounted) return;
      if (jpeg == null) return _say(l.profileErrAvatarType);
      if (!await c.setAvatar(jpeg) && mounted) _say(_errorText(l, c.error ?? 'error'));
    } on AvatarPickerException catch (e) {
      if (mounted) _say(_errorText(l, e.code));
    }
  }

  Future<void> _remove() async {
    final c = _c;
    if (c == null) return;
    final l = AppLocalizations.of(context);
    if (!await confirmRemoval(context, title: l.profileRemovePhotoTitle, text: l.profileRemovePhotoAsk, action: l.downloadRemove)) return;
    if (!await c.removeAvatar() && mounted) _say(_errorText(l, c.error ?? 'error'));
  }

  Future<void> _save() async {
    final c = _c;
    if (c == null) return;
    final l = AppLocalizations.of(context);
    final ok = await c.save(nickname: _nick.text.trim(), firstName: _first.text.trim(), lastName: _last.text.trim(), language: _language);
    if (!mounted) return;
    _say(ok ? l.profileSaved : _errorText(l, c.error ?? 'error'));
  }

  Future<void> _changePin() async {
    final c = _c;
    if (c == null) return;
    final l = AppLocalizations.of(context);
    if (_new.text != _repeat.text) {
      setState(() => _pinError = l.profileErrPinMismatch);
      return;
    }
    setState(() => _pinError = null);
    final ok = await c.changePin(_current.text, _new.text);
    if (!mounted) return;
    if (ok) {
      _current.clear();
      _new.clear();
      _repeat.clear();
      _say(l.profilePinChanged);
    } else {
      setState(() => _pinError = _errorText(l, c.error ?? 'error'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final c = _c;
    final profile = c?.profile;
    if (profile != null) _fill(profile);
    return Scaffold(
      appBar: AppBar(title: Text(l.profileTitle), backgroundColor: Colors.transparent),
      bottomNavigationBar: const MiniPlayer(),
      body: c == null || profile == null
          ? Center(child: Padding(padding: const EdgeInsets.all(24), child: Text(l.profileErrOffline, textAlign: TextAlign.center)))
          : ListenableBuilder(
              listenable: c,
              builder: (context, _) {
                final canEdit = c.available && !c.busy;
                final picker = c.picker ?? const PlatformAvatarPicker();
                return Center(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 560),
                    child: ListView(
                      padding: const EdgeInsets.all(16),
                      children: [
                        if (!c.available)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 12),
                            child: Text(l.profileErrOffline, key: const ValueKey('profile-offline'), style: text.bodyMedium?.copyWith(color: cs.error)),
                          ),
                        Center(child: AvatarCircle(bytes: c.avatarBytes, name: profile.name, size: 112)),
                        const SizedBox(height: 12),
                        Wrap(
                          alignment: WrapAlignment.center,
                          spacing: 8,
                          runSpacing: 8,
                          children: [
                            if (picker.hasCamera)
                              FilledButton.icon(
                                key: const ValueKey('avatar-camera'),
                                onPressed: canEdit ? () => _pick(AvatarSource.camera) : null,
                                icon: const Icon(Icons.photo_camera_outlined),
                                label: Text(l.profileTakePhoto),
                              ),
                            OutlinedButton.icon(
                              key: const ValueKey('avatar-gallery'),
                              onPressed: canEdit ? () => _pick(AvatarSource.gallery) : null,
                              icon: const Icon(Icons.photo_library_outlined),
                              label: Text(l.profileChoosePhoto),
                            ),
                            if (profile.hasAvatar)
                              TextButton.icon(
                                key: const ValueKey('avatar-remove'),
                                onPressed: canEdit ? _remove : null,
                                icon: const Icon(Icons.delete_outline),
                                label: Text(l.profileRemovePhoto),
                              ),
                          ],
                        ),
                        const SizedBox(height: 24),
                        TextField(
                          key: const ValueKey('profile-nickname'),
                          controller: _nick,
                          enabled: canEdit,
                          maxLength: 80,
                          decoration: InputDecoration(labelText: l.profileNickname),
                        ),
                        TextField(
                          key: const ValueKey('profile-first'),
                          controller: _first,
                          enabled: canEdit,
                          maxLength: 80,
                          decoration: InputDecoration(labelText: l.profileFirstName),
                        ),
                        TextField(
                          key: const ValueKey('profile-last'),
                          controller: _last,
                          enabled: canEdit,
                          maxLength: 80,
                          decoration: InputDecoration(labelText: l.profileLastName),
                        ),
                        const SizedBox(height: 8),
                        DropdownButtonFormField<String>(
                          key: const ValueKey('profile-language'),
                          initialValue: _language,
                          decoration: InputDecoration(labelText: l.profileLanguage),
                          items: [
                            DropdownMenuItem(value: '', child: Text(l.profileLanguageAuto)),
                            const DropdownMenuItem(value: 'en', child: Text('English')),
                            const DropdownMenuItem(value: 'de', child: Text('Deutsch')),
                          ],
                          onChanged: canEdit ? (v) => setState(() => _language = v ?? '') : null,
                        ),
                        const SizedBox(height: 8),
                        ListTile(
                          contentPadding: EdgeInsets.zero,
                          title: Text(l.profileRole),
                          subtitle: Text(_roleName(l, profile.role), key: const ValueKey('profile-role')),
                        ),
                        const SizedBox(height: 8),
                        Align(
                          alignment: Alignment.centerRight,
                          child: FilledButton(
                            key: const ValueKey('profile-save'),
                            onPressed: canEdit && _nick.text.trim().isNotEmpty ? _save : null,
                            child: Text(l.profileSave),
                          ),
                        ),
                        const SizedBox(height: 24),
                        Text(l.profilePinTitle, style: text.titleMedium?.copyWith(color: cs.primary)),
                        const SizedBox(height: 8),
                        TextField(
                          key: const ValueKey('pin-current'),
                          controller: _current,
                          enabled: canEdit,
                          obscureText: true,
                          decoration: InputDecoration(labelText: l.profilePinCurrent),
                        ),
                        TextField(
                          key: const ValueKey('pin-new'),
                          controller: _new,
                          enabled: canEdit,
                          obscureText: true,
                          decoration: InputDecoration(labelText: l.profilePinNew),
                        ),
                        TextField(
                          key: const ValueKey('pin-repeat'),
                          controller: _repeat,
                          enabled: canEdit,
                          obscureText: true,
                          decoration: InputDecoration(labelText: l.profilePinRepeat, errorText: _pinError),
                        ),
                        const SizedBox(height: 8),
                        Align(
                          alignment: Alignment.centerRight,
                          child: FilledButton.tonal(
                            key: const ValueKey('pin-save'),
                            onPressed: canEdit && _current.text.isNotEmpty && _new.text.isNotEmpty ? _changePin : null,
                            child: Text(l.profileSave),
                          ),
                        ),
                        const SizedBox(height: 24),
                        Text(l.profileDevices, style: text.titleMedium?.copyWith(color: cs.primary)),
                        const SizedBox(height: 4),
                        Text(
                          profile.deviceLimit == null
                              ? l.profileDeviceCountFree(profile.deviceCount)
                              : l.profileDeviceCount(profile.deviceCount, profile.deviceLimit!),
                          style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                        ),
                        for (final d in c.devices)
                          ListTile(
                            key: ValueKey('device-${d.id}'),
                            contentPadding: EdgeInsets.zero,
                            leading: const Icon(Icons.devices_outlined),
                            title: Text(d.isThis ? '${d.name} (${l.profileDeviceThis})' : d.name),
                            subtitle: Text(d.lastSeen == null ? l.profileNeverSeen : l.profileLastSeen(MaterialLocalizations.of(context).formatShortDate(d.lastSeen!))),
                          ),
                      ],
                    ),
                  ),
                );
              },
            ),
    );
  }
}
