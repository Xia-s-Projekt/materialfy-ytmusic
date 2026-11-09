// Devices — зеркало DevicesService (Spotify Connect): модель устройств, активное, перенос, громкость устройства.
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::{DevicesEvent, DevicesSnapshot};

use super::{Delivery, forward_events, forward_watch, model_ptr, to_qmap, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_rows};
use crate::qt::QVariantMap;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(*mut QObject, devices, READ = devices_ptr, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, active, READ, NOTIFY = changed)]
        #[qproperty(bool, loading, READ, NOTIFY = changed)]
        #[qproperty(QString, transferring_to, cxx_name = "transferringTo", READ, NOTIFY = changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = changed)]
        #[qproperty(QString, this_device_id, cxx_name = "thisDeviceId", READ, NOTIFY = changed)]
        #[qproperty(QString, this_device_name, cxx_name = "thisDeviceName", READ, NOTIFY = changed)]
        type Devices = super::DevicesRust;

        fn devices_ptr(self: &Devices) -> *mut QObject;

        #[qsignal]
        fn changed(self: Pin<&mut Devices>);
        /// ошибки приёмников и списка («Не удалось подключить …») — Snackbar; «{имя} не ответил» — Player.notice
        #[qsignal]
        fn notice(self: Pin<&mut Devices>, text: QString);

        /// Запросить список (при открытии меню и по кнопке обновления; периодического таймера нет).
        #[qinvokable]
        fn refresh(self: &Devices);
        /// Перенести воспроизведение на устройство (id этого ПК — сюда).
        #[qinvokable]
        fn transfer(self: &Devices, device_id: &QString);
        #[qinvokable]
        #[cxx_name = "transferHere"]
        fn transfer_here(self: &Devices);
        /// Громкость устройства 0..100.
        #[qinvokable]
        #[cxx_name = "setVolume"]
        fn set_volume(self: &Devices, device_id: &QString, percent: i32);
        /// Подключить приёмник из локальной сети (source = "zeroconf") по имени.
        #[qinvokable]
        #[cxx_name = "activateReceiver"]
        fn activate_receiver(self: &Devices, name: &QString);
    }

    impl cxx_qt::Threading for Devices {}
    impl cxx_qt::Initialize for Devices {}
}

pub struct DevicesRust {
    snapshot: DevicesSnapshot,
    devices: UniquePtr<DictListModel>,
    active: QVariantMap,
    loading: bool,
    transferring_to: QString,
    error_text: QString,
    this_device_id: QString,
    this_device_name: QString,
}

impl Default for DevicesRust {
    fn default() -> Self {
        Self {
            snapshot: DevicesSnapshot::default(),
            devices: DictListModel::create(ModelKind::Device),
            active: QVariantMap::default(),
            loading: false,
            transferring_to: QString::default(),
            error_text: QString::default(),
            this_device_id: QString::default(),
            this_device_name: QString::default(),
        }
    }
}

impl qobject::Devices {
    fn devices_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.devices)
    }

    fn refresh(&self) {
        ctx().services.devices.refresh();
    }

    fn transfer(&self, device_id: &QString) {
        ctx().services.devices.transfer(device_id.to_string(), true);
    }

    fn transfer_here(&self) {
        ctx().services.devices.transfer_here();
    }

    fn set_volume(&self, device_id: &QString, percent: i32) {
        ctx().services.devices.set_volume(device_id.to_string(), percent.clamp(0, 100) as u8);
    }

    fn activate_receiver(&self, name: &QString) {
        ctx().services.devices.activate_receiver(name.to_string());
    }

    fn apply(mut self: Pin<&mut Self>, next: DevicesSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        if !Arc::ptr_eq(&prev.devices, &next.devices) && prev.devices != next.devices {
            with_model(&mut self.as_mut().rust_mut().get_mut().devices, |m| m.sync_rows(to_rows(&next.devices), "id"));
        }
        let mut this = self.as_mut().rust_mut();
        this.active = next.active.as_ref().map(to_qmap).unwrap_or_default();
        this.loading = next.loading;
        this.transferring_to = QString::from(next.transferring_to.as_str());
        this.error_text = QString::from(next.error_text.as_str());
        this.this_device_id = QString::from(next.this_device_id.as_str());
        this.this_device_name = QString::from(next.this_device_name.as_str());
        self.changed();
    }

    fn on_event(self: Pin<&mut Self>, delivery: Delivery<DevicesEvent>) {
        if let Delivery::Event(DevicesEvent::Notice(text)) = delivery {
            self.notice(QString::from(text.as_str()));
        }
    }
}

impl cxx_qt::Initialize for qobject::Devices {
    fn initialize(mut self: Pin<&mut Self>) {
        let devices = &ctx().services.devices;
        let mut watch = devices.watch();
        let snapshot = watch.borrow_and_update().clone();
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::apply);
        forward_events(self.qt_thread(), devices.subscribe(), Self::on_event);
    }
}
