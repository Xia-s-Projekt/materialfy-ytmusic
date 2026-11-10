import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Settings → "Updates": current version and the result of the last check, "Check now" (with a spinner right
// inside the button), automatic checks, channel (stable / beta). The update itself lives in UpdateSheet.
SettingsCard {
    id: root

    property bool compact: false

    readonly property string st: Updater.state
    readonly property bool hasOffer: Updater.version !== ""

    readonly property string status: {
        switch (st) {
        case "checking": return I18n.t("Checking for updates…")
        case "upToDate": return I18n.t("You’re up to date")
        case "available": return Updater.skipped ? I18n.t("Version {0} is available (skipped)", Updater.version)
                                                 : I18n.t("Version {0} is available", Updater.version)
        case "downloading": return I18n.t("Downloading version {0} · {1} %", Updater.version, Math.floor(Updater.progress * 100))
        case "verifying": return I18n.t("Verifying version {0}…", Updater.version)
        case "readyToInstall": return I18n.t("Version {0} is ready to install", Updater.version)
        case "installing": return I18n.t("Installing version {0}…", Updater.version)
        case "error": return Updater.errorText
        default: return Updater.autoCheck ? I18n.t("Not checked yet") : I18n.t("Automatic checks are off")
        }
    }

    function _lower(text) { return text.charAt(0).toLowerCase() + text.slice(1) }

    title: I18n.t("Updates")
    icon: "download"

    SettingRow {
        title: I18n.t("Materialfy {0}", Updater.currentVersion)
        description: root.status
                     + (Updater.lastChecked > 0 && root.st !== "checking" && root.st !== "error"
                        ? " · " + I18n.t("checked {0}", root._lower(I18n.relative(Updater.lastChecked))) : "")

        Row {
            spacing: 8

            LoadingIndicator {
                visible: root.st === "checking"
                size: root.compact ? 32 : 40
                anchors.verticalCenter: parent.verticalCenter
            }
            Button {
                enabled: root.st !== "checking" && root.st !== "downloading" && root.st !== "verifying"
                         && root.st !== "installing" && root.st !== "readyToInstall"
                text: root.st === "checking" ? I18n.t("Checking…") : I18n.t("Check now")
                icon: root.st === "checking" ? "" : "refresh"
                style: root.hasOffer ? "outlined" : "tonal"
                size: root.compact ? "xs" : "s"
                anchors.verticalCenter: parent.verticalCenter
                onClicked: Updater.check()
            }
            Button {
                visible: root.hasOffer && root.st !== "checking"
                text: root.st === "readyToInstall" ? I18n.t("Install") : I18n.t("View update")
                size: root.compact ? "xs" : "s"
                anchors.verticalCenter: parent.verticalCenter
                onClicked: Updater.showSheet()
            }
        }
    }

    SettingRow {
        title: I18n.t("Check for updates automatically")
        description: I18n.t("At startup and every 6 hours. Only the public release list on GitHub is requested")
        Switch { checked: Updater.autoCheck; onToggled: c => Updater.autoCheck = c }
    }

    SettingRow {
        title: I18n.t("Update channel")
        description: Updater.channel === "beta"
                     ? I18n.t("Beta: pre-release versions too. New features arrive earlier, but may be less stable")
                     : I18n.t("Stable: only finished releases")
        ButtonGroup {
            model: [I18n.t("Stable"), I18n.t("Beta")]
            autoSelect: false
            size: root.compact ? "xs" : "s"
            currentIndex: Updater.channel === "beta" ? 1 : 0
            onActivated: i => Updater.channel = ["stable", "beta"][i]
        }
    }
}
