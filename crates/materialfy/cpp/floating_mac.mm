// The island on macOS: a window above the menu bar (its level is NSMainMenuWindowLevel, Qt's usual
// "always on top" is lower), on every desktop but not over full-screen apps (they get their own
// desktop), and it does not hide when the app is inactive (Qt::Tool is an NSPanel with
// hidesOnDeactivate). The screen notch size lives in the window properties notchW / notchH
// (FloatingIsland.qml); without a notch it is 0: the island is then as on Windows, a strip at the edge.
#include "floating.h"

#import <AppKit/AppKit.h>

#include <QtGui/QGuiApplication>
#include <QtGui/QWindow>

#include <cmath>

namespace mf {

bool
floating_tool_window(QObject* object)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || QGuiApplication::platformName() != QLatin1String("cocoa"))
    return false;
  NSView* view = reinterpret_cast<NSView*>(window->winId());
  NSWindow* ns = view.window;
  if (!ns)
    return false;
  ns.level = NSMainMenuWindowLevel + 1;
  ns.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorStationary
                          | NSWindowCollectionBehaviorIgnoresCycle;
  ns.hidesOnDeactivate = NO;
  ns.hasShadow = NO;

  int notch_w = 0;
  int notch_h = 0;
  if (@available(macOS 12.0, *)) {
    NSScreen* screen = ns.screen;
    if (screen && screen.safeAreaInsets.top > 0) {
      // the notch — between the two visible pieces of the menu bar, left and right of it
      const NSRect l = screen.auxiliaryTopLeftArea;
      const NSRect r = screen.auxiliaryTopRightArea;
      notch_h = static_cast<int>(std::lround(screen.safeAreaInsets.top));
      notch_w = static_cast<int>(std::lround(NSWidth(screen.frame) - NSWidth(l) - NSWidth(r)));
    }
  }
  window->setProperty("notchW", notch_w);
  window->setProperty("notchH", notch_h);
  return true;
}

} // namespace mf
