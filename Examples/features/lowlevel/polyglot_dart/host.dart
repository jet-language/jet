import 'dart:ffi';
import 'dart:io';

import '.jet/bindings/dart/callbacks_host.dart';

typedef ComputeNative = Int64 Function(Int64);
typedef ComputeDart = int Function(int);
typedef ComputeFloatNative = Double Function(Double);
typedef ComputeFloatDart = double Function(double);

String nativePath() {
  final extension = Platform.isMacOS
      ? 'dylib'
      : Platform.isWindows
      ? 'dll'
      : 'so';
  return '.jet/bindings/dart/libjet_dart_callbacks_compute.$extension';
}

void main() {
  initializeJetDart(nativePath());
  final compute = jetDartLibrary.lookupFunction<ComputeNative, ComputeDart>(
    'compute',
  );
  final computeFloat = jetDartLibrary
      .lookupFunction<ComputeFloatNative, ComputeFloatDart>('compute-float');
  print(compute(21));
  print(computeFloat(5.0));
  shutdownJetDart();
}
