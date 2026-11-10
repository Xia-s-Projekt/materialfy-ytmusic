//! serde_json::Value <-> QVariant. Dicts from QML (track, playlist) arrive as QVariantMap.
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QMetaTypeType, QString, QStringList, QVariant};
use serde_json::{Map, Number, Value};

pub type QVariantMap = QMap<QMapPair_QString_QVariant>;
pub type QVariantList = QList<QVariant>;

pub fn json_to_qvariant(value: &Value) -> QVariant {
    match value {
        Value::Null => QVariant::default(),
        Value::Bool(b) => QVariant::from(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match i32::try_from(i) {
                    Ok(small) => QVariant::from(&small),
                    Err(_) => QVariant::from(&i),
                }
            } else if let Some(u) = n.as_u64() {
                QVariant::from(&u)
            } else {
                QVariant::from(&n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => QVariant::from(&QString::from(s.as_str())),
        Value::Array(items) => QVariant::from(&json_to_qvariantlist(items)),
        Value::Object(map) => QVariant::from(&json_to_qvariantmap(map)),
    }
}

pub fn json_to_qvariantmap(map: &Map<String, Value>) -> QVariantMap {
    let mut out = QVariantMap::default();
    for (key, value) in map {
        out.insert(QString::from(key.as_str()), json_to_qvariant(value));
    }
    out
}

pub fn json_to_qvariantlist(items: &[Value]) -> QVariantList {
    let mut out = QVariantList::default();
    for item in items {
        out.append(json_to_qvariant(item));
    }
    out
}

pub fn qvariant_to_json(value: &QVariant) -> Value {
    match value.type_id() {
        QMetaTypeType::UnknownType => Value::Null,
        QMetaTypeType::Bool => Value::Bool(value.value::<bool>().unwrap_or_default()),
        QMetaTypeType::Int | QMetaTypeType::Short | QMetaTypeType::Long | QMetaTypeType::SChar => {
            Value::from(value.value::<i32>().unwrap_or_default())
        }
        QMetaTypeType::UInt | QMetaTypeType::UShort | QMetaTypeType::ULong | QMetaTypeType::UChar => {
            Value::from(value.value::<u32>().unwrap_or_default())
        }
        QMetaTypeType::LongLong => Value::from(value.value::<i64>().unwrap_or_default()),
        QMetaTypeType::ULongLong => Value::from(value.value::<u64>().unwrap_or_default()),
        QMetaTypeType::Double | QMetaTypeType::Float => {
            let f = value.value::<f64>().unwrap_or_default();
            Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
        }
        QMetaTypeType::QString => Value::String(value.value::<QString>().unwrap_or_default().to_string()),
        QMetaTypeType::QStringList => {
            let list = value.value::<QStringList>().unwrap_or_default();
            let list: QList<QString> = QList::from(&list);
            Value::Array(list.iter().map(|s| Value::String(s.to_string())).collect())
        }
        QMetaTypeType::QVariantMap => {
            Value::Object(qvariantmap_to_json(&value.value::<QVariantMap>().unwrap_or_default()))
        }
        QMetaTypeType::QVariantList => {
            let list = value.value::<QVariantList>().unwrap_or_default();
            Value::Array(list.iter().map(qvariant_to_json).collect())
        }
        // other types (QUrl, QColor...) - as a string, if Qt can show them that way
        _ => match value.value::<QString>() {
            Some(s) => Value::String(s.to_string()),
            None => Value::Null,
        },
    }
}

pub fn qvariantmap_to_json(map: &QVariantMap) -> Map<String, Value> {
    map.iter().map(|(k, v)| (k.to_string(), qvariant_to_json(v))).collect()
}
