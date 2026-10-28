//! The AACP channel: an L2CAP `BluetoothSocket` with a blocking read thread.

use std::sync::Arc;

use anyhow::Context as _;
use jni::Env;
use jni::objects::JObject;
use jni::refs::Global;
use parking_lot::Mutex;

use crate::jni::android::bluetooth::adapter::BluetoothAdapter;
use crate::jni::android::bluetooth::device::BluetoothDevice;
use crate::jni::android::bluetooth::socket::BluetoothSocket;
use crate::jni::android::os::parcel_uuid::ParcelUuid;
use crate::jni::java::lang::boolean::Boolean;
use crate::jni::java::lang::integer::Integer;
use crate::jni::{Class as _, GetDeclaredConstructors as _, with_env};
use crate::proto::connection::{AIRPODS_PSM, AIRPODS_UUID};

/// Android's hidden `BluetoothSocket.TYPE_L2CAP`.
pub(crate) const TYPE_L2CAP: i32 = 3;

/// "No existing fd" for the socket constructors that take one.
pub(crate) const NO_FD: i32 = -1;

#[derive(Debug, Clone)]
pub(crate) enum BluetoothEvent {
    Connected,
    Received(Vec<u8>),
    /// The socket failed or closed.
    Closed(String),
}

pub(crate) type Socket = BluetoothSocket<Global<JObject<'static>>>;

/// The current link. `generation` bumps on every connect/disconnect, so a
/// stale connect attempt or read loop can tell it has been superseded.
pub(crate) struct Link {
    generation: u64,
    socket: Option<Arc<Socket>>,
}

pub(crate) static LINK: Mutex<Link> = Mutex::new(Link {
    generation: 0,
    socket: None,
});

/// Open the AACP socket to `mac` on a background thread, reporting through
/// `on_event` until the link is replaced or closed.
pub(crate) fn connect(mac: String, on_event: impl Fn(BluetoothEvent) + Send + 'static) {
    let generation = {
        let mut link = LINK.lock();
        link.generation += 1;
        link.generation
    };
    let is_current = move || LINK.lock().generation == generation;

    std::thread::spawn(move || {
        let socket = match open_socket(&mac) {
            Ok(socket) => Arc::new(socket),
            Err(e) => return on_event(BluetoothEvent::Closed(format!("{e:#}"))),
        };
        {
            let mut link = LINK.lock();
            if link.generation != generation {
                drop(link);
                close(&socket);
                return;
            }
            link.socket = Some(socket.clone());
        }
        on_event(BluetoothEvent::Connected);

        // Don't use `available()`: it reports 0 for L2CAP even with data pending.
        let result = with_env(|env| {
            let input = socket.get_input_stream(env)?;
            while is_current() {
                let data = input.read(env)?;
                if !data.is_empty() {
                    on_event(BluetoothEvent::Received(data));
                }
            }
            Ok(())
        });
        if let Err(e) = result
            && is_current()
        {
            on_event(BluetoothEvent::Closed(format!("{e:#}")));
        }
    });
}

/// Close the current link; its read loop ends without reporting.
pub(crate) fn disconnect() {
    let socket = {
        let mut link = LINK.lock();
        link.generation += 1;
        link.socket.take()
    };
    if let Some(socket) = socket {
        close(&socket);
    }
}

pub(crate) fn send(data: &[u8]) -> anyhow::Result<()> {
    let socket = LINK.lock().socket.clone().context("not connected")?;
    crate::proto::protocol::trace_tx(data);
    with_env(|env| socket.get_output_stream(env)?.write(env, data))
}

pub(crate) fn close(socket: &Socket) {
    if let Err(e) = with_env(|env| socket.close(env)) {
        tracing::warn!("closing socket failed: {e:#}");
    }
}

pub(crate) fn open_socket(mac: &str) -> anyhow::Result<Socket> {
    tracing::info!(mac, "connecting AACP");
    with_env(|env| {
        let adapter = BluetoothAdapter::default_adapter(env)?;
        let device = adapter.get_remote_device(env, mac)?;
        let socket = create_l2cap_socket(env, &adapter, &device)?.into_global(env)?;
        socket.connect(env)?;
        Ok(socket)
    })
}

/// The L2CAP `BluetoothSocket` constructor is hidden and its signature varies
/// across Android releases, so try every known argument order via reflection.
pub(crate) fn create_l2cap_socket<'local>(
    env: &mut Env<'local>,
    adapter: &BluetoothAdapter<JObject<'_>>,
    device: &BluetoothDevice<JObject<'_>>,
) -> anyhow::Result<BluetoothSocket<JObject<'local>>> {
    let uuid = ParcelUuid::from_string(env, AIRPODS_UUID)?.0;
    let l2cap = Integer::value_of(env, TYPE_L2CAP)?.0;
    let fd = Integer::value_of(env, NO_FD)?.0;
    let yes = Boolean::value_of(env, true)?.0;
    let psm = Integer::value_of(env, i32::from(AIRPODS_PSM))?.0;
    let (adapter, device) = (&adapter.0, &device.0);

    let signatures: [&[&JObject]; 5] = [
        // Android 17: the adapter first.
        &[adapter, device, &l2cap, &yes, &yes, &psm, &uuid],
        &[device, &l2cap, &yes, &yes, &psm, &uuid],
        &[device, &l2cap, &fd, &yes, &yes, &psm, &uuid],
        &[&l2cap, &fd, &yes, &yes, device, &psm, &uuid],
        &[&l2cap, &yes, &yes, device, &psm, &uuid],
    ];
    let constructors = BluetoothSocket::<JObject>::class(env)?.get_declared_constructors(env)?;

    for args in signatures {
        for constructor in &constructors {
            if constructor.get_parameter_count(env)? as usize != args.len() {
                continue;
            }
            constructor.set_accessible(env, true)?;
            match constructor.new_instance(env, args) {
                Ok(socket) if !socket.is_null() => return Ok(BluetoothSocket(socket)),
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("socket constructor rejected: {e}");
                    env.exception_clear();
                }
            }
        }
    }
    anyhow::bail!("no BluetoothSocket constructor accepted L2CAP arguments")
}
