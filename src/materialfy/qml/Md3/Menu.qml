import QtQuick
import QtQuick.Shapes
import QtQuick.Templates as T

// MD3 popup menu: a card with an item list at a point in the window. A click outside or Esc closes it.
// The menu is a separate popup window (Popup.Window): it extends past the edges of the app window (in the mini player
// 280×120 it would not otherwise fit) and is clamped to the screen edges. A submenu lives in the same window, next to the card:
// Qt gives the mouse to the last open popup window, and in a separate window the main card went dead for submenus.
// Placed in a window: Menu { anchors.fill: parent }; open(items, point[, size]) — the point is in window coordinates.
// Item: { id, text, icon, enabled (true), checked (undefined — no checkmark), hint (grey text on the right),
// separator: true, submenu: [items] }.
// picked(index) — a top-level item was chosen (its list index); triggered(id, item) — any chosen item.
Item {
    id: root

    property var items: []
    readonly property bool opened: mainPopup.visible

    signal picked(int index)
    signal triggered(string id, var item)

    // padding for the card shadow inside the transparent window
    readonly property int _pad: 12

    // size — the size of the button the menu opened from (point — its top-left corner): the card is placed
    // under the button, right edge to its right edge, and if it does not fit below — above it. Without size the menu opens
    // from the point (the cursor on right-click) down-right and flips left/up at the edge.
    function open(list, point, size) {
        closeSub()
        items = list
        main.currentIndex = -1
        const g = root.mapToGlobal(point.x, point.y)
        mainPopup.anchor = Qt.rect(g.x, g.y, size ? size.width : 0, size ? size.height : 0)
        mainPopup.area = _screenArea(g)
        mainPopup.windowArea = _windowArea()
        mainPopup.open()
        main.forceActiveFocus()
    }
    function close() {
        closeSub()
        mainPopup.close()
    }
    function closeSub() {
        hoverTimer.stop()
        sub.parentIndex = -1
        _subOpen = false
    }
    property bool _subOpen: false

    // The screen under the anchor (a window can span two monitors) in global coordinates; the taskbar
    // at the bottom/right is cut off by the available desktop area (the screen's availableGeometry is not visible in QML)
    function _screenArea(g) {
        // Wayland does not report where the window sits on screen: mapToGlobal gives coordinates inside the window, while the geometry
        // of the monitor is global (the second monitor does not start at zero), and the menu drifted. There the boundary is the window itself.
        if (Qt.platform.pluginName.startsWith("wayland"))
            return _windowArea()
        const w = root.Window.window
        let s = w ? w.screen : null
        for (const sc of Qt.application.screens) {
            if (g.x >= sc.virtualX && g.x < sc.virtualX + sc.width && g.y >= sc.virtualY && g.y < sc.virtualY + sc.height) {
                s = sc
                break
            }
        }
        if (!s || !(s.width > 0)) {
            const o = root.mapToGlobal(0, 0)
            return Qt.rect(o.x, o.y, root.width, root.height)
        }
        let right = s.virtualX + s.width
        let bottom = s.virtualY + s.height
        if (s.desktopAvailableWidth > s.virtualX)
            right = Math.min(right, s.desktopAvailableWidth)
        if (s.desktopAvailableHeight > s.virtualY)
            bottom = Math.min(bottom, s.desktopAvailableHeight)
        return Qt.rect(s.virtualX, s.virtualY, right - s.virtualX, bottom - s.virtualY)
    }

    // The app window in global coordinates: the menu stays inside it when it fits
    function _windowArea() {
        const w = root.Window.window
        if (!w || !w.contentItem)
            return Qt.rect(0, 0, 0, 0)
        const o = w.contentItem.mapToGlobal(0, 0)
        return Qt.rect(o.x, o.y, w.width, w.height)
    }

    function _usable(item) {
        return item && !item.separator && item.enabled !== false
    }

    function _openSub(index, focusFirst) {
        hoverTimer.stop()
        const item = items[index]
        if (!_usable(item) || !item.submenu || item.submenu.length === 0)
            return
        if (sub.parentIndex !== index || !_subOpen) {
            sub.model = item.submenu
            sub.parentIndex = index
            sub.currentIndex = -1
            const row = main.rowAt(index)
            mainPopup.subRowY = row ? row.mapToItem(main, 0, 0).y : 0
            _subOpen = true
            subEnter.restart()
        }
        if (focusFirst) {
            sub.currentIndex = sub.step(-1, 1)
            sub.forceActiveFocus()
        }
    }

    function _activate(card, index) {
        const item = card.model[index]
        if (!_usable(item))
            return
        if (card.level === 0 && item.submenu && item.submenu.length > 0) {
            _openSub(index, true)
            return
        }
        close()
        if (card.level === 0)
            picked(index)
        triggered(item.id ?? "", item)
    }

    // The app window goes to the background — the menu closes (like system menus)
    Connections {
        target: Qt.application
        function onStateChanged() {
            if (Qt.application.state !== Qt.ApplicationActive)
                root.close()
        }
    }

    // Delay before opening/closing a hovered submenu: a diagonal mouse path to the submenu does not collapse it
    Timer {
        id: hoverTimer
        property int index: -1
        interval: 160
        onTriggered: {
            const item = root.items[index]
            if (item && item.submenu && item.submenu.length > 0)
                root._openSub(index, false)
            else
                root.closeSub()
        }
    }

    // A popup window for a single card: transparent, with padding for the shadow; position in global coordinates
    component CardPopup: T.Popup {
        id: pop
        property rect area: Qt.rect(0, 0, 0, 0)
        property Item card: null

        // On Wayland — inside the window: the compositor moves separate popup windows itself, and the menu ended up
        // away from the button. Boundaries there are still per window (see _screenArea).
        popupType: Qt.platform.pluginName.startsWith("wayland") ? T.Popup.Item : T.Popup.Window
        modal: false
        dim: false
        focus: true
        padding: root._pad
        background: null
        // the popup's x/y are relative to root; computed in global coordinates and converted
        function toLocal(gx, gy) { return root.mapFromGlobal(gx, gy) }

        // the window size follows the card (the container's implicit size does not reach Popup.Window)
        contentWidth: card ? card.implicitWidth : 0
        contentHeight: card ? card.implicitHeight : 0
        // in Templates (no style) the popup size is not tied to its content
        implicitWidth: contentWidth + leftPadding + rightPadding
        implicitHeight: contentHeight + topPadding + bottomPadding

        enter: Transition {
            ParallelAnimation {
                NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motion.effectsFast }
                NumberAnimation { property: "scale"; from: 0.9; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.OutBack }
            }
        }
        exit: Transition {
            NumberAnimation { property: "opacity"; from: 1; to: 0; duration: Theme.motion.effectsFast }
        }
    }

    component MenuCard: Rectangle {
        id: card

        property var model: []
        property int level: 0
        property int currentIndex: -1
        property int parentIndex: -1   // for a submenu: the parent item
        readonly property bool hasIcons: (model || []).some(it => (it.icon ?? "") !== "")
        readonly property bool hasTrailing: (model || []).some(it => typeof it.checked === "boolean" || (it.submenu && it.submenu.length > 0))

        function rowAt(i) { return rows.itemAt(i) }
        // The next enabled item from from in direction dir (skipping separators and disabled ones)
        function step(from, dir) {
            const n = (model || []).length
            let i = from < 0 ? (dir > 0 ? -1 : n) : from
            for (let k = 0; k < n; ++k) {
                i = ((i + dir) % n + n) % n
                if (root._usable(model[i]))
                    return i
            }
            return -1
        }

        implicitWidth: list.implicitWidth + 16
        implicitHeight: list.implicitHeight + 16
        width: implicitWidth
        height: implicitHeight
        radius: Theme.shape.large
        color: Qt.alpha(Theme.surfaceContainer, 0.96)
        border.width: 1
        border.color: Qt.alpha(Theme.outlineVariant, 0.5)

        Keys.onPressed: event => {
            switch (event.key) {
            case Qt.Key_Down:
                currentIndex = step(currentIndex, 1)
                break
            case Qt.Key_Up:
                currentIndex = step(currentIndex, -1)
                break
            case Qt.Key_Right:
                if (level === 0 && currentIndex >= 0)
                    root._openSub(currentIndex, true)
                break
            case Qt.Key_Left:
                if (level === 1) {
                    root.closeSub()
                    main.forceActiveFocus()
                }
                break
            case Qt.Key_Return:
            case Qt.Key_Enter:
            case Qt.Key_Space:
                if (currentIndex >= 0)
                    root._activate(card, currentIndex)
                break
            case Qt.Key_Escape:
                if (level === 1) {
                    root.closeSub()
                    main.forceActiveFocus()
                } else {
                    root.close()
                }
                break
            default:
                return
            }
            event.accepted = true
        }

        Elevation { target: card; level: 2 }

        Column {
            id: list
            x: 8
            y: 8

            Repeater {
                id: rows
                model: card.model

                Item {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool isSeparator: modelData.separator === true
                    readonly property bool usable: root._usable(modelData)
                    readonly property bool hasSub: !isSeparator && card.level === 0 && !!modelData.submenu && modelData.submenu.length > 0
                    readonly property bool current: card.currentIndex === index
                    readonly property real contentLeft: 12 + (card.hasIcons ? 20 + 12 : 0)

                    width: isSeparator ? 1 : Math.max(204, contentLeft + label.implicitWidth + (card.hasTrailing ? 16 + 20 : 0)
                                                     + (hintLabel.visible ? 16 + hintLabel.implicitWidth : 0) + 12)
                    height: isSeparator ? 9 : 40

                    Rectangle {
                        visible: row.isSeparator
                        x: 4
                        width: list.width - 8
                        height: 1
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.outlineVariant
                        opacity: 0.6
                    }

                    // Highlight: the current keyboard item and the open submenu parent
                    Rectangle {
                        visible: !row.isSeparator
                        width: list.width
                        height: parent.height
                        radius: Theme.shape.medium
                        color: Theme.fgSurface
                        opacity: (row.current && !hit.containsMouse) || (row.hasSub && root._subOpen && sub.parentIndex === row.index)
                                 ? Theme.stateLayer.focus : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                    }

                    Item {
                        visible: !row.isSeparator
                        width: list.width
                        height: parent.height
                        opacity: row.usable ? 1 : Theme.stateLayer.disabledContent

                        Icon {
                            visible: (row.modelData.icon ?? "") !== ""
                            x: 12
                            anchors.verticalCenter: parent.verticalCenter
                            name: row.modelData.icon ?? ""
                            size: 20
                            color: Theme.fgSurfaceVariant
                        }
                        Label {
                            id: label
                            x: row.contentLeft
                            anchors.verticalCenter: parent.verticalCenter
                            text: row.modelData.text ?? ""
                            type: "bodyMedium"
                            color: Theme.fgSurface
                        }
                        // the hint on the right in grey ("local")
                        Label {
                            id: hintLabel
                            visible: (row.modelData.hint ?? "") !== ""
                            anchors.right: parent.right
                            anchors.rightMargin: 12 + (card.hasTrailing ? 20 + 8 : 0)
                            anchors.verticalCenter: parent.verticalCenter
                            text: row.modelData.hint ?? ""
                            type: "labelSmall"
                            color: Theme.fgSurfaceVariant
                        }
                        Icon {
                            visible: row.modelData.checked === true
                            x: parent.width - 12 - 20
                            anchors.verticalCenter: parent.verticalCenter
                            name: "check"
                            size: 20
                            color: Theme.primary
                        }
                        // The "›" chevron: the reduced icon font has no chevron_right
                        Shape {
                            visible: row.hasSub
                            x: parent.width - 12 - 20
                            width: 20
                            height: 20
                            anchors.verticalCenter: parent.verticalCenter
                            preferredRendererType: Shape.CurveRenderer
                            ShapePath {
                                strokeColor: Theme.fgSurfaceVariant
                                strokeWidth: 1.8
                                fillColor: "transparent"
                                capStyle: ShapePath.RoundCap
                                joinStyle: ShapePath.RoundJoin
                                startX: 8; startY: 5
                                PathLine { x: 13; y: 10 }
                                PathLine { x: 8; y: 15 }
                            }
                        }
                    }

                    StateLayer {
                        id: hit
                        visible: !row.isSeparator
                        enabled: row.usable
                        width: list.width
                        height: parent.height
                        radius: Theme.shape.medium
                        onContainsMouseChanged: {
                            if (!containsMouse)
                                return
                            card.currentIndex = row.index
                            if (card.level === 0) {
                                hoverTimer.index = row.index
                                hoverTimer.restart()
                                card.forceActiveFocus()
                            } else {
                                hoverTimer.stop()
                            }
                        }
                        onClicked: root._activate(card, row.index)
                    }
                }
            }
        }
    }

    // Positions are bindings: the card size is only known after the items are laid out.
    // Boundaries are the app window if the card fits inside it (in the mini player it does not: then the screen).
    // Horizontally: from a button — right edge to its right edge, from a point — to the right; if it does not fit, the other
    // way. Vertically: below the anchor, if it does not fit — above it, and if that does not fit either — wherever there is more room.
    CardPopup {
        id: mainPopup
        property rect anchor: Qt.rect(0, 0, 0, 0)
        property rect windowArea: Qt.rect(0, 0, 0, 0)
        readonly property real gap: anchor.height > 0 ? 4 : 0
        readonly property rect bounds: {
            const a = area, w = windowArea
            if (w.width < main.width + 16 || w.height < main.height + 16)
                return a
            const l = Math.max(a.x, w.x), t = Math.max(a.y, w.y)
            const r = Math.min(a.x + a.width, w.x + w.width), b = Math.min(a.y + a.height, w.y + w.height)
            return r - l >= main.width + 16 && b - t >= main.height + 16 ? Qt.rect(l, t, r - l, b - t) : a
        }
        readonly property bool toLeft: anchor.width > 0
                                       ? anchor.x + anchor.width - main.width >= bounds.x + 8
                                       : anchor.x + main.width > bounds.x + bounds.width - 8
        readonly property bool below: anchor.y + anchor.height + gap + main.height <= bounds.y + bounds.height - 8
                                      || anchor.y - gap - main.height < bounds.y + 8
                                         && bounds.y + bounds.height - anchor.y - anchor.height >= anchor.y - bounds.y
        // the card in global coordinates
        readonly property real mx: Math.max(bounds.x + 8, Math.min(bounds.x + bounds.width - main.width - 8,
                                       toLeft ? anchor.x + anchor.width - main.width : anchor.x))
        readonly property real my: Math.max(bounds.y + 8, Math.min(bounds.y + bounds.height - main.height - 8,
                                       below ? anchor.y + anchor.height + gap : anchor.y - gap - main.height))
        // submenu: to the right of the card, if it does not fit — to the left; vertically at the parent row (subRowY is from the top of the card)
        property real subRowY: 0
        readonly property real sx: mx + main.width - 4 + sub.width > area.x + area.width - 8
                                   ? Math.max(area.x + 8, mx - sub.width + 4)
                                   : mx + main.width - 4
        readonly property real sy: Math.max(area.y + 8, Math.min(my + subRowY - 8, area.y + area.height - sub.height - 8))
        // the window spans both cards
        readonly property real left: root._subOpen ? Math.min(mx, sx) : mx
        readonly property real top: root._subOpen ? Math.min(my, sy) : my
        readonly property real right: root._subOpen ? Math.max(mx + main.width, sx + sub.width) : mx + main.width
        readonly property real bottom: root._subOpen ? Math.max(my + main.height, sy + sub.height) : my + main.height
        x: toLocal(left - root._pad, top - root._pad).x
        y: toLocal(left - root._pad, top - root._pad).y
        closePolicy: T.Popup.CloseOnEscape | T.Popup.CloseOnPressOutside
        // expands from the corner at the anchor
        transformOrigin: below ? (toLeft ? Item.TopRight : Item.TopLeft) : (toLeft ? Item.BottomRight : Item.BottomLeft)
        onClosed: root.closeSub()
        card: pane

        Item {
            id: pane
            implicitWidth: mainPopup.right - mainPopup.left
            implicitHeight: mainPopup.bottom - mainPopup.top

            MenuCard {
                id: main
                x: mainPopup.mx - mainPopup.left
                y: mainPopup.my - mainPopup.top
                level: 0
                model: root.items
            }

            MenuCard {
                id: sub
                x: mainPopup.sx - mainPopup.left
                y: mainPopup.sy - mainPopup.top
                visible: root._subOpen
                level: 1
                transformOrigin: x < main.x ? Item.TopRight : Item.TopLeft
                HoverHandler { onHoveredChanged: if (hovered) hoverTimer.stop() }

                ParallelAnimation {
                    id: subEnter
                    NumberAnimation { target: sub; property: "opacity"; from: 0; to: 1; duration: Theme.motion.effectsFast }
                    NumberAnimation { target: sub; property: "scale"; from: 0.9; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.OutBack }
                }
            }
        }
    }
}
