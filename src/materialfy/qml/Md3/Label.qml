import QtQuick

// Text on the MD3 scale: Label { type: "titleLarge" }. The font is the variable Google Sans:
// weight and optical size are driven by axes, not only by font.weight.
Text {
    id: root

    property string type: "bodyMedium"
    // fade: overflowing text fades out at the edge instead of showing "…". The shader layer is used only while
    // the text is longer than the line (single line, no wrapping).
    property bool fade: false
    // width of the whole line without "…": implicitWidth with elide on can already be clipped
    readonly property real naturalWidth: fade ? _metrics.advanceWidth : implicitWidth
    readonly property bool overflowing: fade && wrapMode === Text.NoWrap && width > 0 && naturalWidth > width + 0.5
    property real fadeLength: 28
    // The fade is no wider than ~15% of the line: in a narrow tile a fixed 28 px ate
    // half a word, and the whole heading looked blurred instead of just its trailing edge.
    readonly property real fadeWidth: Math.min(fadeLength, Math.max(0, width) * 0.15)
    readonly property var _t: Theme.type[type] ?? Theme.type.bodyMedium
    property real weight: _t.weight      // 400..700, wght axis

    color: Theme.fgSurface
    font.family: Theme.fontFamily
    font.pixelSize: _t.size
    // With a variable font weight is axis-only: font.weight makes Qt pick the nearest
    // static file of the family (600 → Medium), and intermediate weights disappear.
    font.weight: Theme.variableFont ? Font.Normal : Math.round(weight)
    font.letterSpacing: _t.tracking
    font.variableAxes: Theme.fontAxes(_t.size, weight)
    lineHeight: _t.line
    lineHeightMode: Text.FixedHeight
    verticalAlignment: Text.AlignVCenter
    elide: overflowing ? Text.ElideNone : Text.ElideRight
    textFormat: Text.PlainText

    TextMetrics {
        id: _metrics
        font: root.font
        text: root.fade ? root.text : ""
    }

    layer.enabled: overflowing
    layer.effect: ShaderEffect {
        readonly property real fadeFrom: Math.max(0, 1 - root.fadeWidth / Math.max(1, root.width))
        fragmentShader: Qt.resolvedUrl("shaders/fadeedge.frag.qsb")
    }
}
