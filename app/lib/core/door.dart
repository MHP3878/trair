// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

/// The narrow door to the Rust core: JSON request in, JSON answer out.
/// The protocol is documented in crates/trair-ffi/src/api.rs.
abstract class CoreDoor {
  Future<Map<String, Object?>> call(Map<String, Object?> request);
}

/// Thrown when the native core cannot be loaded. The app then shows an
/// error and never falls back to demo data.
class CoreMissing implements Exception {
  const CoreMissing(this.message);

  final String message;

  @override
  String toString() => 'CoreMissing: $message';
}

/// The real door: trair_ffi.dll (Windows) or libtrair_ffi.so, loaded with
/// dart:ffi. Every call runs on a background isolate, because unlocking
/// takes about a second on purpose.
class NativeDoor implements CoreDoor {
  NativeDoor._(this._path);

  /// Version of the C interface this app speaks (see abi.rs).
  static const abiVersion = 1;

  final String _path;

  /// Load the library and check its interface version. Without [path]:
  /// TRAIR_FFI_LIB, else the library next to the app.
  static NativeDoor load([String? path]) {
    path ??= Platform.environment['TRAIR_FFI_LIB'] ?? _defaultName();
    try {
      final lib = DynamicLibrary.open(path);
      final version = lib.lookupFunction<Uint32 Function(), int Function()>('trair_abi_version')();
      if (version != abiVersion) {
        throw CoreMissing('Sicherheitskern hat Version $version, die App erwartet $abiVersion.');
      }
    } on ArgumentError catch (e) {
      throw CoreMissing('Sicherheitskern nicht gefunden ($path): ${e.message}');
    }
    return NativeDoor._(path);
  }

  static String _defaultName() {
    if (Platform.isWindows) return 'trair_ffi.dll';
    if (Platform.isMacOS) return 'libtrair_ffi.dylib';
    return 'libtrair_ffi.so';
  }

  @override
  Future<Map<String, Object?>> call(Map<String, Object?> request) async {
    final path = _path;
    final text = jsonEncode(request);
    final answer = await Isolate.run(() => _callSync(path, text));
    final decoded = jsonDecode(answer);
    if (decoded is! Map<String, Object?>) {
      return const {'ok': false, 'error': 'internal'};
    }
    return decoded;
  }
}

String _callSync(String path, String request) {
  final lib = DynamicLibrary.open(path);
  final alloc = lib.lookupFunction<Pointer<Uint8> Function(Size), Pointer<Uint8> Function(int)>('trair_alloc');
  final free = lib.lookupFunction<Void Function(Pointer<Uint8>, Size), void Function(Pointer<Uint8>, int)>(
    'trair_free',
  );
  final call = lib
      .lookupFunction<Pointer<Uint8> Function(Pointer<Uint8>, Size), Pointer<Uint8> Function(Pointer<Uint8>, int)>(
        'trair_call',
      );
  final bytes = utf8.encode(request);
  final input = alloc(bytes.length);
  if (input == nullptr) return '{"ok":false,"error":"bad_request"}';
  input.asTypedList(bytes.length).setAll(0, bytes);
  // Best effort: the request may hold a password. The core wipes its own
  // buffer in trair_free; this wipes the Dart copy of the bytes.
  bytes.fillRange(0, bytes.length, 0);
  final output = call(input, bytes.length);
  free(input, bytes.length);
  if (output == nullptr) return '{"ok":false,"error":"internal"}';
  final length = ByteData.sublistView(output.asTypedList(4)).getUint32(0, Endian.little);
  final answer = utf8.decode(output.asTypedList(4 + length).sublist(4));
  free(output, 4 + length);
  return answer;
}
