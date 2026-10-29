// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:multicast_dns/multicast_dns.dart';

/// A server announced on the local network (`_david._tcp`, spec
/// `spec-discovery-profiles-roles.md`, section 1).
///
/// mDNS is not authenticated. What it finds is a candidate address and nothing more:
/// the connection is accepted only if the server shows the certificate pinned at
/// pairing.
class FoundServer {
  FoundServer({required this.serverId, required this.name, required this.host, required this.port});

  final String serverId;
  final String name;
  final String host;
  final int port;

  /// `host:port`, the form the API client takes.
  String get address => host.contains(':') ? '[$host]:$port' : '$host:$port';
}

const _service = '_david._tcp.local';

/// Looks for servers for [timeout] on every network interface. A server is reported
/// once, however many interfaces it is seen on.
///
/// The query has to leave through the interface of the local network. Left to
/// itself the system sends multicast through the default route, which on a machine
/// with a VPN is the tunnel, and nothing answers. So there is one client per
/// interface, each told where to send (`IP_MULTICAST_IF`).
Future<List<FoundServer>> discoverServers({
  Duration timeout = const Duration(seconds: 4),
  List<InternetAddress>? interfaces,
}) async {
  // iOS only lets an app use multicast sockets with a special permission from Apple (an entitlement that has to
  // be applied for). Without it the search finds nothing; the app then keeps the address it was paired with.
  if (Platform.isIOS && interfaces == null) return const [];
  final addresses = interfaces ?? await _lanAddresses();
  final perInterface = await Future.wait(
    addresses.map((a) => _guarded(() => _discoverOn(a, timeout))),
  );
  final found = <String, FoundServer>{};
  for (final server in perInterface.expand((l) => l)) {
    found.putIfAbsent(server.serverId, () => server);
  }
  return found.values.toList();
}

Future<List<InternetAddress>> _lanAddresses() async => [
  for (final i in await NetworkInterface.list(type: InternetAddressType.IPv4))
    for (final a in i.addresses)
      if (!a.isLoopback) a,
];

/// IP_MULTICAST_IF: 32 on Linux and Android, 9 on macOS, iOS and Windows.
int get _multicastIf => Platform.isLinux || Platform.isAndroid ? 32 : 9;

/// Runs one interface's search. A tunnel interface can refuse the send with an
/// error that `multicast_dns` raises outside the search's own future; it must
/// neither end the app nor the search on the other interfaces.
Future<List<FoundServer>> _guarded(Future<List<FoundServer>> Function() search) {
  final done = Completer<List<FoundServer>>();
  runZonedGuarded(
    () => search().then(done.complete, onError: (Object _) => done.complete(const [])),
    (error, stack) {
      if (!done.isCompleted && error is! SocketException) done.completeError(error, stack);
    },
  );
  return done.future;
}

Future<List<FoundServer>> _discoverOn(InternetAddress local, Duration timeout) async {
  final mdns = MDnsClient(
    rawDatagramSocketFactory: (host, port, {bool? reuseAddress, bool? reusePort, int? ttl}) async {
      final socket = await RawDatagramSocket.bind(
        host,
        port,
        reuseAddress: true,
        reusePort: reusePort ?? true,
        ttl: ttl ?? 1,
      );
      try {
        socket.setRawOption(
          RawSocketOption(RawSocketOption.levelIPv4, _multicastIf, local.rawAddress),
        );
      } on OSError {
        // Without it the query goes through the default route; it may still work.
      }
      return socket;
    },
  );
  final found = <FoundServer>[];
  try {
    await mdns.start();
    final deadline = DateTime.now().add(timeout);
    await for (final ptr in mdns.lookup<PtrResourceRecord>(
      ResourceRecordQuery.serverPointer(_service),
      timeout: timeout,
    )) {
      final left = deadline.difference(DateTime.now());
      if (left <= Duration.zero) break;
      final server = await _resolve(mdns, ptr.domainName, left);
      if (server != null) found.add(server);
    }
  } on SocketException {
    // An interface that cannot do mDNS (a tunnel, a bridge that is down) finds nothing.
  } finally {
    mdns.stop();
  }
  return found;
}

/// The server with this id, or null if it does not answer in time.
Future<FoundServer?> findServer(
  String serverId, {
  Duration timeout = const Duration(seconds: 4),
}) async {
  for (final s in await discoverServers(timeout: timeout)) {
    if (s.serverId == serverId) return s;
  }
  return null;
}

Future<FoundServer?> _resolve(MDnsClient mdns, String instance, Duration timeout) async {
  final srv = await _first(
    mdns.lookup<SrvResourceRecord>(ResourceRecordQuery.service(instance), timeout: timeout),
  );
  if (srv == null) return null;
  final txt = await _first(
    mdns.lookup<TxtResourceRecord>(ResourceRecordQuery.text(instance), timeout: timeout),
  );
  final fields = parseTxt(txt?.text ?? '');
  final id = fields['id'];
  if (id == null || id.isEmpty) return null;
  final ip =
      await _first(
        mdns.lookup<IPAddressResourceRecord>(
          ResourceRecordQuery.addressIPv4(srv.target),
          timeout: timeout,
        ),
      ) ??
      await _first(
        mdns.lookup<IPAddressResourceRecord>(
          ResourceRecordQuery.addressIPv6(srv.target),
          timeout: timeout,
        ),
      );
  if (ip == null) return null;
  return FoundServer(
    serverId: id,
    name: fields['name'] ?? '',
    host: ip.address.address,
    port: srv.port,
  );
}

Future<T?> _first<T>(Stream<T> stream) async {
  await for (final v in stream) {
    return v;
  }
  return null;
}

/// The text records `key=value`, one per line as `multicast_dns` delivers them.
Map<String, String> parseTxt(String text) {
  final out = <String, String>{};
  for (final line in const LineSplitter().convert(text)) {
    final i = line.indexOf('=');
    if (i > 0) out[line.substring(0, i)] = line.substring(i + 1);
  }
  return out;
}
