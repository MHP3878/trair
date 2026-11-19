// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';

/// A QR code, computed by the Rust core and drawn here square by square:
/// dark squares on a light card, so phone cameras read it easily.
class QrBox extends StatefulWidget {
  const QrBox({super.key, required this.text, this.size = 140});

  final String text;
  final double size;

  @override
  State<QrBox> createState() => _QrBoxState();
}

class _QrBoxState extends State<QrBox> {
  List<String> _rows = const [];
  String _for = '';

  Future<void> _load(AppState state) async {
    final text = widget.text;
    if (text.isEmpty || text == _for) return;
    _for = text;
    final q = await state.vault?.qr(text);
    if (!mounted || q == null || !q.ok) return;
    setState(() => _rows = (q.raw['rows'] as List).cast<String>());
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    _load(state);
    return Semantics(
      label: 'QR-Code des Kontaktcodes',
      child: Container(
        width: widget.size,
        height: widget.size,
        padding: const EdgeInsets.all(10),
        decoration: BoxDecoration(
          color: const Color(0xFFF4F1FF),
          borderRadius: BorderRadius.circular(18),
          boxShadow: const [BoxShadow(color: Color(0x40A18DFF), blurRadius: 24)],
        ),
        child: _rows.isEmpty
            ? const Center(child: Text('QR', style: TextStyle(color: TrairColors.placeholder)))
            : CustomPaint(painter: _QrPainter(_rows)),
      ),
    );
  }
}

class _QrPainter extends CustomPainter {
  _QrPainter(this.rows);

  final List<String> rows;

  @override
  void paint(Canvas canvas, Size size) {
    final n = rows.length;
    final cell = size.shortestSide / n;
    final paint = Paint()..color = const Color(0xFF14101F);
    for (var y = 0; y < n; y++) {
      final row = rows[y];
      for (var x = 0; x < row.length; x++) {
        if (row.codeUnitAt(x) == 0x31) {
          // Slightly larger than one cell, so no hairlines show between.
          canvas.drawRect(Rect.fromLTWH(x * cell, y * cell, cell + 0.3, cell + 0.3), paint);
        }
      }
    }
  }

  @override
  bool shouldRepaint(_QrPainter old) => old.rows != rows;
}
