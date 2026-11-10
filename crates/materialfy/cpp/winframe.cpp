// A native window frame without chrome on Windows (WM_NCCALCSIZE + WM_NCHITTEST). Owner — the "shell" agent.
// The window style gets WS_THICKFRAME and company back (Qt removed them because of FramelessWindowHint):
// smooth native resize, Aero Snap, Win+arrows, animations, a DWM shadow. Windows does not draw chrome —
// the whole area is client area. Dragging is in QML (TopBar → startSystemMove, it needs WS_SYSMENU),
// we do not return HTCAPTION: the sidebar and buttons live in the same strip.
#include "winframe.h"

#include <QtGui/QWindow>

namespace mf {

void
window_minimize(QObject* object)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window)
    return;
  // Qt on Windows sets WPF_RESTORETOMAXIMIZED from the new state's Maximized flag; FullScreen stays —
  // the window comes back from the taskbar fullscreen
  window->setWindowStates(window->windowStates() | Qt::WindowMinimized);
}

} // namespace mf

#if defined(_WIN32)

#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <windowsx.h>

#include <QtCore/QAbstractNativeEventFilter>
#include <QtCore/QByteArray>
#include <QtCore/QCoreApplication>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtGui/QGuiApplication>
#include <QtGui/QWindow>

#include <algorithm>

namespace mf {

namespace {

// WS_THICKFRAME and company: native resize, Aero Snap, Win+arrows, animations
void
apply_frame_style(HWND hwnd)
{
  const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
  const LONG_PTR want = style | WS_THICKFRAME | WS_CAPTION | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_SYSMENU;
  if (want == style)
    return;
  SetWindowLongPtrW(hwnd, GWL_STYLE, want);
  SetWindowPos(hwnd, nullptr, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

// Windows 11: MD3 rounded corners. DWM draws them itself (together with a thin outline) and makes them square on a maximized
// window. In fullscreen (F11) the window is ordinary by style (we restore WS_THICKFRAME ourselves) — DWM would have left the corners and outline,
// so there they are turned off explicitly. Windows 10 does not know these attributes — the calls return an error, the corners are square.
void
apply_corners(HWND hwnd, bool fullscreen)
{
  constexpr DWORD corner_preference = 33; // DWMWA_WINDOW_CORNER_PREFERENCE (SDK 10.0.22000+)
  constexpr DWORD border_color = 34;      // DWMWA_BORDER_COLOR
  const int corners = fullscreen ? 1 : 2; // DWMWCP_DONOTROUND : DWMWCP_ROUND
  const COLORREF border = fullscreen ? 0xFFFFFFFE : 0xFFFFFFFF; // DWMWA_COLOR_NONE : DWMWA_COLOR_DEFAULT
  DwmSetWindowAttribute(hwnd, corner_preference, &corners, sizeof(corners));
  DwmSetWindowAttribute(hwnd, border_color, &border, sizeof(border));
}

bool
taskbar_autohide()
{
  APPBARDATA data{};
  data.cbSize = sizeof(data);
  return (SHAppBarMessage(ABM_GETSTATE, &data) & ABS_AUTOHIDE) != 0;
}

class FrameFilter : public QAbstractNativeEventFilter
{
public:
  FrameFilter(QWindow* window, HWND hwnd, int grip)
    : m_window(window)
    , m_hwnd(hwnd)
    , m_grip(grip)
  {
  }

  bool nativeEventFilter(const QByteArray& type, void* message, qintptr* result) override
  {
    if (type != "windows_generic_MSG")
      return false;
    auto* msg = static_cast<MSG*>(message);
    if (msg->hwnd != m_hwnd)
      return false;
    switch (msg->message) {
      case WM_NCCALCSIZE:
        if (!msg->wParam)
          return false;
        if (IsZoomed(m_hwnd))
          fit_maximized(reinterpret_cast<NCCALCSIZE_PARAMS*>(msg->lParam)->rgrc[0]);
        *result = 0;
        return true;
      case WM_NCHITTEST:
        *result = hit_test(GET_X_LPARAM(msg->lParam), GET_Y_LPARAM(msg->lParam));
        return true;
      case WM_STYLECHANGED:
        // Qt recomputes the style from the window flags (for example, "always on top") and removes WS_THICKFRAME —
        // we restore it after Qt finishes
        if (msg->wParam == static_cast<WPARAM>(GWL_STYLE)
            && !(reinterpret_cast<const STYLESTRUCT*>(msg->lParam)->styleNew & WS_THICKFRAME)) {
          QMetaObject::invokeMethod(
            QCoreApplication::instance(), [hwnd = m_hwnd] { apply_frame_style(hwnd); }, Qt::QueuedConnection);
        }
        return false;
      default:
        return false;
    }
  }

private:
  // A maximized window with WS_THICKFRAME is placed by Windows past the monitor edges by the frame thickness — we clip
  // the client area to the monitor's work area (if Qt already constrained the window to it, nothing changes).
  void fit_maximized(RECT& rect) const
  {
    MONITORINFO info{};
    info.cbSize = sizeof(info);
    if (!GetMonitorInfoW(MonitorFromWindow(m_hwnd, MONITOR_DEFAULTTONEAREST), &info))
      return;
    RECT work = info.rcWork;
    // An auto-hiding taskbar does not slide over a full-monitor window — we leave it 1 px
    if (EqualRect(&info.rcWork, &info.rcMonitor) && taskbar_autohide())
      work.bottom -= 1;
    rect.left = std::max(rect.left, work.left);
    rect.top = std::max(rect.top, work.top);
    rect.right = std::min(rect.right, work.right);
    rect.bottom = std::min(rect.bottom, work.bottom);
  }

  LRESULT hit_test(int x, int y) const
  {
    if (IsZoomed(m_hwnd) || (m_window && (m_window->windowStates() & Qt::WindowFullScreen)))
      return HTCLIENT;
    RECT rect;
    if (!GetWindowRect(m_hwnd, &rect))
      return HTCLIENT;
    const UINT dpi = GetDpiForWindow(m_hwnd);
    const int g = MulDiv(m_grip, dpi ? static_cast<int>(dpi) : 96, 96);
    const bool left = x - rect.left < g;
    const bool right = rect.right - x <= g;
    const bool top = y - rect.top < g;
    const bool bottom = rect.bottom - y <= g;
    if (top && left)
      return HTTOPLEFT;
    if (top && right)
      return HTTOPRIGHT;
    if (bottom && left)
      return HTBOTTOMLEFT;
    if (bottom && right)
      return HTBOTTOMRIGHT;
    if (left)
      return HTLEFT;
    if (right)
      return HTRIGHT;
    if (top)
      return HTTOP;
    if (bottom)
      return HTBOTTOM;
    return HTCLIENT;
  }

  QPointer<QWindow> m_window;
  HWND m_hwnd;
  int m_grip;
};

} // namespace

bool
winframe_install(QObject* object, int grip)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window)
    return false;
  auto hwnd = reinterpret_cast<HWND>(window->winId());
  if (!hwnd)
    return false;

  // The filter comes before SetWindowPos: it has to catch the WM_NCCALCSIZE from SWP_FRAMECHANGED.
  // It lives until the process ends (the window is the main one).
  QGuiApplication::instance()->installNativeEventFilter(new FrameFilter(window, hwnd, grip));

  apply_frame_style(hwnd);

  // The DWM shadow (Windows 10 and 11)
  const MARGINS margins{ 0, 0, 0, 1 };
  DwmExtendFrameIntoClientArea(hwnd, &margins);

  apply_corners(hwnd, window->windowStates().testFlag(Qt::WindowFullScreen));
  // A minimized window does not change corners: from the taskbar it comes back in its previous state
  QObject::connect(window, &QWindow::windowStateChanged, window, [hwnd](Qt::WindowState state) {
    if (state != Qt::WindowMinimized)
      apply_corners(hwnd, state == Qt::WindowFullScreen);
  });
  return true;
}

} // namespace mf

#else

namespace mf {

bool
winframe_install(QObject* object, int grip)
{
  (void)object;
  (void)grip;
  return false;
}

} // namespace mf

#endif
