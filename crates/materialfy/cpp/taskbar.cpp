// Playback buttons under the window thumbnail on the Windows taskbar (issue #2). Owner — the "shell" agent.
// ITaskbarList3::ThumbBarAddButtons on the main window's HWND; a click reaches the window as WM_COMMAND with
// HIWORD(wParam) = THBN_CLICKED — caught by the native event filter. The state comes from the Player singleton
// (playing → "Pause", no track → the buttons are inactive), the labels come from Translations.tr (mf_i18n), the glyphs from the
// Material Symbols font, light on a dark taskbar and dark on a light one. Explorer recreates the window button
// (a restart, the window shown again from the tray) — it sends "TaskbarButtonCreated", and then the buttons are added again.
#include "taskbar.h"

#if defined(_WIN32)

#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <shobjidl.h>

#include <QtCore/QAbstractNativeEventFilter>
#include <QtCore/QByteArray>
#include <QtCore/QCoreApplication>
#include <QtCore/QPointer>
#include <QtGui/QColor>
#include <QtGui/QFont>
#include <QtGui/QGuiApplication>
#include <QtGui/QImage>
#include <QtGui/QPainter>
#include <QtGui/QPainterPath>
#include <QtGui/QWindow>
#include <QtQml/QQmlEngine>

#include <algorithm>
#include <array>
#include <cmath>
#include <cwchar>
#include <string>
#include <utility>

#include "materialfy/src/bridges/player.cxxqt.h"
#include "materialfy/src/bridges/translations.cxxqt.h"

namespace mf {

namespace {

enum Button : UINT
{
  Previous = 1,
  PlayPause = 2,
  Next = 3,
};

// Material Symbols code points (assets/fonts/icons.json; the font is trimmed, these glyphs are in it — the player uses them)
constexpr char16_t glyph_previous = 0xE045; // skip_previous
constexpr char16_t glyph_play = 0xE037;     // play_arrow
constexpr char16_t glyph_pause = 0xE034;    // pause
constexpr char16_t glyph_next = 0xE044;     // skip_next

// The taskbar is light (Settings → Personalization → Colors → "Windows mode")
bool
taskbar_light()
{
  DWORD value = 0;
  DWORD size = sizeof(value);
  const LSTATUS status = RegGetValueW(HKEY_CURRENT_USER,
                                      L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                                      L"SystemUsesLightTheme",
                                      RRF_RT_REG_DWORD,
                                      nullptr,
                                      &value,
                                      &size);
  return status == ERROR_SUCCESS && value != 0;
}

// A filled glyph centered in a size×size square (by the outline, not by the font cell)
HICON
make_icon(char16_t code, int size, const QColor& color)
{
  QImage image(size, size, QImage::Format_ARGB32_Premultiplied);
  image.fill(Qt::transparent);
  QFont font(QStringLiteral("Material Symbols Rounded"));
  // The glyphs are drawn in a 24 grid with padding: play_arrow is ~14/24 tall — so it takes up ~3/4 of the icon
  font.setPixelSize(std::max(8, static_cast<int>(std::lround(size * 1.25))));
  font.setVariableAxis(QFont::Tag("FILL"), 1.0f);
  font.setVariableAxis(QFont::Tag("opsz"), 20.0f);
  QPainterPath path;
  path.addText(0, 0, font, QString(QChar(code)));
  const QRectF ink = path.boundingRect();
  if (!ink.isEmpty()) {
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.translate(std::round((size - ink.width()) / 2 - ink.left()), std::round((size - ink.height()) / 2 - ink.top()));
    painter.fillPath(path, color);
  }
  return image.toHICON();
}

class ThumbBar : public QAbstractNativeEventFilter
{
public:
  ThumbBar(HWND hwnd, Player* player, Translations* translations)
    : m_hwnd(hwnd)
    , m_player(player)
    , m_translations(translations)
    , m_created(RegisterWindowMessageW(L"TaskbarButtonCreated"))
  {
    // An elevated window would not otherwise receive messages from Explorer
    ChangeWindowMessageFilterEx(m_hwnd, m_created, MSGFLT_ALLOW, nullptr);
    ChangeWindowMessageFilterEx(m_hwnd, WM_COMMAND, MSGFLT_ALLOW, nullptr);

    auto* context = QCoreApplication::instance();
    const auto refresh = [this] { update(); };
    QObject::connect(player, &Player::stateChanged, context, refresh);
    QObject::connect(player, &Player::trackChanged, context, refresh);
    if (translations)
      QObject::connect(translations, &Translations::languageChanged, context, refresh);
    // The window button on the taskbar may already have appeared (the window was shown before the filter was installed) —
    // then TaskbarButtonCreated will not arrive; if it has not appeared, adding will fail and will be retried on it
    add();
  }

  bool nativeEventFilter(const QByteArray& type, void* message, qintptr* result) override
  {
    if (type != "windows_generic_MSG")
      return false;
    auto* msg = static_cast<MSG*>(message);
    if (msg->hwnd != m_hwnd)
      return false;
    if (msg->message == m_created && m_created) {
      // A new button (Explorer restarted, the window shown again) — the old object and buttons are invalid
      reset();
      add();
      return false;
    }
    switch (msg->message) {
      case WM_COMMAND:
        if (HIWORD(msg->wParam) != THBN_CLICKED)
          return false;
        clicked(LOWORD(msg->wParam));
        // result is sometimes zero (Qt passes messages that did not come through the window procedure this way)
        if (result)
          *result = 0;
        return true;
      case WM_SETTINGCHANGE:
        // The Windows theme changed (light / dark taskbar) — redraw the glyphs
        if (msg->lParam && std::wcscmp(reinterpret_cast<const wchar_t*>(msg->lParam), L"ImmersiveColorSet") == 0) {
          QMetaObject::invokeMethod(QCoreApplication::instance(), [this] { update(true); }, Qt::QueuedConnection);
        }
        return false;
      case WM_DPICHANGED:
      case WM_DISPLAYCHANGE:
        QMetaObject::invokeMethod(QCoreApplication::instance(), [this] { update(true); }, Qt::QueuedConnection);
        return false;
      default:
        return false;
    }
  }

private:
  void clicked(UINT id) const
  {
    if (!m_player || !m_player->getHasTrack())
      return;
    switch (id) {
      case Previous:
        m_player->previous();
        break;
      case PlayPause:
        m_player->togglePlay();
        break;
      case Next:
        m_player->next();
        break;
      default:
        break;
    }
  }

  void reset()
  {
    if (m_list)
      m_list->Release();
    m_list = nullptr;
    m_added = false;
  }

  bool ensure_list()
  {
    if (m_list)
      return true;
    ITaskbarList3* list = nullptr;
    if (FAILED(CoCreateInstance(CLSID_TaskbarList, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&list))))
      return false;
    if (FAILED(list->HrInit())) {
      list->Release();
      return false;
    }
    m_list = list;
    return true;
  }

  // Icons for the current size (SM_CXSMICON at the taskbar's DPI) and the taskbar color
  void ensure_icons(bool force)
  {
    const int size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
    const bool light = taskbar_light();
    if (!force && size == m_icon_size && light == m_light && m_icons[0])
      return;
    // The old icons live until the next ThumbBarUpdateButtons (the taskbar may not have picked up a copy yet)
    drop_old_icons();
    m_old_icons = m_icons;
    m_icon_size = size;
    m_light = light;
    const QColor color = light ? QColor(0x1f, 0x1f, 0x1f) : QColor(Qt::white);
    m_icons = { make_icon(glyph_previous, size, color),
                make_icon(glyph_play, size, color),
                make_icon(glyph_pause, size, color),
                make_icon(glyph_next, size, color) };
  }

  void drop_old_icons()
  {
    for (HICON& icon : m_old_icons) {
      if (icon)
        DestroyIcon(icon);
      icon = nullptr;
    }
  }

  QString tr(const char* text) const
  {
    const QString en = QString::fromUtf8(text);
    return m_translations ? m_translations->tr(en) : en;
  }

  std::array<THUMBBUTTON, 3> buttons()
  {
    const bool enabled = m_player && m_player->getHasTrack();
    const bool playing = m_player && m_player->getPlaying();
    // The labels are the same as in the tray icon menu (Main.qml)
    const std::array<std::pair<HICON, QString>, 3> spec{ {
      { m_icons[0], tr("Previous track") },
      { playing ? m_icons[2] : m_icons[1], playing ? tr("Pause") : tr("Resume") },
      { m_icons[3], tr("Next track") },
    } };
    std::array<THUMBBUTTON, 3> out{};
    for (size_t i = 0; i < out.size(); ++i) {
      THUMBBUTTON& b = out[i];
      b.dwMask = THB_ICON | THB_TOOLTIP | THB_FLAGS;
      b.iId = static_cast<UINT>(Previous + i);
      b.hIcon = spec[i].first;
      const std::wstring tip = spec[i].second.toStdWString();
      wcsncpy_s(b.szTip, tip.c_str(), _TRUNCATE);
      b.dwFlags = enabled ? THBF_ENABLED : THBF_DISABLED;
    }
    return out;
  }

  void add()
  {
    if (!ensure_list())
      return;
    ensure_icons(false);
    auto list = buttons();
    // The buttons on the window already exist (a second call without recreating the window button) — Add will return an error, so we update them
    m_added = SUCCEEDED(m_list->ThumbBarAddButtons(m_hwnd, static_cast<UINT>(list.size()), list.data()))
              || SUCCEEDED(m_list->ThumbBarUpdateButtons(m_hwnd, static_cast<UINT>(list.size()), list.data()));
    if (m_added)
      drop_old_icons();
  }

  void update(bool restyle = false)
  {
    if (!m_added) {
      add();
      return;
    }
    ensure_icons(restyle);
    auto list = buttons();
    if (SUCCEEDED(m_list->ThumbBarUpdateButtons(m_hwnd, static_cast<UINT>(list.size()), list.data())))
      drop_old_icons();
  }

  HWND m_hwnd;
  QPointer<Player> m_player;
  QPointer<Translations> m_translations;
  UINT m_created;
  ITaskbarList3* m_list = nullptr;
  bool m_added = false;
  // previous, play, pause, next
  std::array<HICON, 4> m_icons{};
  std::array<HICON, 4> m_old_icons{};
  int m_icon_size = 0;
  bool m_light = false;
};

} // namespace

bool
taskbar_install(QObject* object)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window)
    return false;
  auto hwnd = reinterpret_cast<HWND>(window->winId());
  QQmlEngine* engine = qmlEngine(object);
  if (!hwnd || !engine)
    return false;
  auto* player = engine->singletonInstance<Player*>("Materialfy.Core", "Player");
  if (!player)
    return false;
  auto* translations = engine->singletonInstance<Translations*>("Materialfy.Core", "Translations");
  // It lives until the process ends (the window is the main one).
  QGuiApplication::instance()->installNativeEventFilter(new ThumbBar(hwnd, player, translations));
  return true;
}

} // namespace mf

#else

namespace mf {

bool
taskbar_install(QObject* window)
{
  (void)window;
  return false;
}

} // namespace mf

#endif
