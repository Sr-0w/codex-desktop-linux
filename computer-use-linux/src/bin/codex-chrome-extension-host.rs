use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{self, Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const HOST_NAME: &str = "com.openai.codexextension";
const SOCKET_DIR_ENV: &str = "CODEX_BROWSER_USE_SOCKET_DIR";
const SESSIONS_DIR_ENV: &str = "CODEX_BROWSER_USE_SESSIONS_DIR";
const DEFAULT_SOCKET_DIR: &str = "/tmp/codex-browser-use";
const RUNTIME_CONFIG_FILE: &str = "extension-host-config.json";
const RUNTIME_CONFIG_PATH_ENV: &str = "CODEX_EXTENSION_HOST_CONFIG_PATH";
const MANIFEST_SCHEMA_VERSION: u64 = 2;
const NATIVE_HOST_PROTOCOL_VERSION: u64 = 2;
const APP_SERVER_PROTOCOL_VERSION: u64 = 2;
const APP_SERVER_START_TIMEOUT: Duration = Duration::from_secs(8);
const APP_SERVER_CONNECT_INTERVAL: Duration = Duration::from_millis(50);
const APP_SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(2);
const APP_SERVER_PROXY_ACCEPT_INTERVAL: Duration = Duration::from_millis(20);
const APP_SERVER_PROXY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const APP_SERVER_PROXY_MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_TAB_CONTEXT_ASSET_BYTES: u64 = 256 * 1024 * 1024;
const ROLLOUT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const OBSERVED_TURN_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const ROLLOUT_SEARCH_MAX_DEPTH: usize = 5;

type SharedState = Arc<Mutex<HostState>>;
type SharedChromeWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type SharedClientWriter = Arc<Mutex<UnixStream>>;

#[derive(Clone)]
struct Client {
    writer: SharedClientWriter,
}

struct PendingChromeRequest {
    client_id: usize,
    client_request_id: Value,
    fallback_extension_info: bool,
}

#[derive(Clone)]
struct PendingClientRequest {
    client_id: usize,
    chrome_request_id: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionHostConfig {
    #[serde(default = "default_runtime_config_schema")]
    schema_version: u64,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    browser_client_path: Option<PathBuf>,
    codex_cli_path: PathBuf,
    #[serde(default)]
    node_module_dirs: Vec<PathBuf>,
    node_path: PathBuf,
    node_repl_path: PathBuf,
    #[serde(default = "default_proxy_host")]
    proxy_host: String,
    #[serde(default)]
    proxy_port: u16,
    #[serde(default)]
    codex_home: Option<PathBuf>,
    #[serde(default)]
    resources_path: Option<PathBuf>,
    #[serde(skip)]
    trusted_browser_client_sha256s: Vec<String>,
}

struct AppServerProcess {
    child: Child,
    url: String,
    proxy: AppServerProxy,
}

impl AppServerProcess {
    fn stop(mut self) {
        self.proxy.stop();
        stop_app_server_child(&mut self.child);
    }
}

struct AppServerProxy {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    listener_thread: Option<thread::JoinHandle<()>>,
}

impl AppServerProxy {
    fn start(
        listener: TcpListener,
        backend_address: SocketAddr,
        allowed_origin: Option<String>,
    ) -> Result<Self> {
        let address = listener
            .local_addr()
            .context("failed to inspect Codex app-server proxy address")?;
        listener
            .set_nonblocking(true)
            .context("failed to make Codex app-server proxy nonblocking")?;

        let stop = Arc::new(AtomicBool::new(false));
        let listener_stop = Arc::clone(&stop);
        let listener_thread = thread::Builder::new()
            .name("codex-app-server-proxy".to_string())
            .spawn(move || {
                run_app_server_proxy_listener(
                    listener,
                    backend_address,
                    allowed_origin,
                    listener_stop,
                )
            })
            .context("failed to spawn Codex app-server proxy listener")?;

        Ok(Self {
            address,
            stop,
            listener_thread: Some(listener_thread),
        })
    }

    fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Acquire)
            && self
                .listener_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, APP_SERVER_CONNECT_INTERVAL);
        if let Some(listener_thread) = self.listener_thread.take() {
            let _ = listener_thread.join();
        }
    }
}

struct TabContextAsset {
    file: Option<File>,
    path: PathBuf,
    written: u64,
}

#[derive(Debug)]
struct RuntimeRequestError {
    error_type: &'static str,
    message: String,
}

impl RuntimeRequestError {
    fn new(error_type: &'static str, message: impl Into<String>) -> Self {
        Self {
            error_type,
            message: message.into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ChromeClientRouteError {
    NoClients,
    MultipleClients,
}

impl ChromeClientRouteError {
    fn message(&self) -> &'static str {
        match self {
            Self::NoClients => "No Codex browser client is connected",
            Self::MultipleClients => {
                "Multiple Codex browser clients are connected; Chrome requests require exactly one"
            }
        }
    }
}

struct HostState {
    stdout: SharedChromeWriter,
    rollout_tracker: RolloutTracker,
    extension_id: Option<String>,
    runtime_config: Option<ExtensionHostConfig>,
    runtime_config_error: Option<String>,
    app_server: Option<AppServerProcess>,
    asset_root: Option<PathBuf>,
    assets: HashMap<String, TabContextAsset>,
    next_asset_id: u64,
    clients: HashMap<usize, Client>,
    pending_chrome_requests: HashMap<String, PendingChromeRequest>,
    pending_client_requests: HashMap<String, PendingClientRequest>,
    next_client_id: usize,
    next_chrome_id: u64,
    next_client_request_id: u64,
}

impl HostState {
    #[cfg(test)]
    fn new(
        stdout: SharedChromeWriter,
        rollout_tracker: RolloutTracker,
        extension_id: Option<String>,
    ) -> Self {
        Self::with_runtime_config(stdout, rollout_tracker, extension_id, None, None)
    }

    fn with_runtime_config(
        stdout: SharedChromeWriter,
        rollout_tracker: RolloutTracker,
        extension_id: Option<String>,
        runtime_config: Option<ExtensionHostConfig>,
        runtime_config_error: Option<String>,
    ) -> Self {
        Self {
            stdout,
            rollout_tracker,
            extension_id,
            runtime_config,
            runtime_config_error,
            app_server: None,
            asset_root: None,
            assets: HashMap::new(),
            next_asset_id: 1,
            clients: HashMap::new(),
            pending_chrome_requests: HashMap::new(),
            pending_client_requests: HashMap::new(),
            next_client_id: 1,
            next_chrome_id: 1,
            next_client_request_id: 1,
        }
    }

    fn stop_runtime(&mut self) {
        if let Some(server) = self.app_server.take() {
            server.stop();
        }
        self.assets.clear();
        if let Some(root) = self.asset_root.take() {
            let _ = fs::remove_dir_all(root);
        }
    }

    fn replace_with_client(&mut self, writer: SharedClientWriter) -> (usize, Vec<(usize, Client)>) {
        let evicted_clients = self.clients.drain().collect::<Vec<_>>();
        if !evicted_clients.is_empty() {
            self.pending_chrome_requests.clear();
            self.pending_client_requests.clear();
        }

        let id = self.next_client_id;
        self.next_client_id += 1;
        self.clients.insert(id, Client { writer });
        (id, evicted_clients)
    }

    fn remove_client(&mut self, client_id: usize) {
        self.clients.remove(&client_id);
        remove_pending_requests_for_client(
            &mut self.pending_chrome_requests,
            &mut self.pending_client_requests,
            client_id,
        );
    }

    fn send_chrome(&self, message: &Value) {
        let mut stdout = self.stdout.lock().expect("stdout mutex poisoned");
        if let Err(error) = write_frame(&mut *stdout, message) {
            log(&format!("native stdout error: {error}"));
            process::exit(1);
        }
    }

    fn send_client(&self, client_id: usize, message: &Value) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };

        let mut writer = client.writer.lock().expect("client writer mutex poisoned");
        if let Err(error) = write_frame(&mut *writer, message) {
            log(&format!("client socket write error: {error}"));
        }
    }

    fn broadcast_clients(&self, message: &Value) {
        for client_id in self.clients.keys().copied().collect::<Vec<_>>() {
            self.send_client(client_id, message);
        }
    }
}

#[derive(Clone)]
struct RolloutTracker {
    inner: Arc<Mutex<RolloutTrackerState>>,
    stdout: SharedChromeWriter,
    sessions_root: Option<PathBuf>,
}

struct RolloutTrackerState {
    observed: HashMap<String, ObservedTurn>,
}

struct ObservedTurn {
    session_id: String,
    turn_id: String,
    path: Option<PathBuf>,
    offset: u64,
    created_at: Instant,
}

impl RolloutTracker {
    fn new(stdout: SharedChromeWriter) -> Self {
        let tracker = Self {
            inner: Arc::new(Mutex::new(RolloutTrackerState {
                observed: HashMap::new(),
            })),
            stdout,
            sessions_root: sessions_root(),
        };

        let worker = tracker.clone();
        if let Err(error) = thread::Builder::new()
            .name("codex-rollout-tracker".to_string())
            .spawn(move || worker.watch_loop())
        {
            log(&format!("extension-host: rollout watcher error: {error}"));
        }

        tracker
    }

    fn observe_request(&self, message: &Value) {
        let Some((session_id, turn_id)) = session_turn_from_message(message) else {
            return;
        };

        let key = observed_turn_key(&session_id, &turn_id);
        let mut state = self.inner.lock().expect("rollout watcher mutex poisoned");
        if state.observed.contains_key(&key) {
            return;
        }

        let (path, offset) = self
            .sessions_root
            .as_deref()
            .and_then(|root| find_rollout_path(root, &session_id))
            .map(|path| {
                let offset = file_len(&path).unwrap_or_default();
                (Some(path), offset)
            })
            .unwrap_or((None, 0));

        state.observed.insert(
            key,
            ObservedTurn {
                session_id,
                turn_id,
                path,
                offset,
                created_at: Instant::now(),
            },
        );
    }

    fn watch_loop(self) {
        loop {
            thread::sleep(ROLLOUT_POLL_INTERVAL);
            if let Err(error) = self.process_rollouts() {
                log(&format!("extension-host: rollout watcher error: {error}"));
            }
        }
    }

    fn process_rollouts(&self) -> Result<()> {
        let Some(sessions_root) = self.sessions_root.as_deref() else {
            return Ok(());
        };

        let mut completed = Vec::new();
        let mut expired = Vec::new();
        {
            let mut state = self.inner.lock().expect("tracker mutex poisoned");
            for (key, observed) in &mut state.observed {
                if observed.created_at.elapsed() >= OBSERVED_TURN_TTL {
                    expired.push(key.clone());
                    continue;
                }

                if observed.path.is_none() {
                    if let Some(path) = find_rollout_path(sessions_root, &observed.session_id) {
                        observed.offset = 0;
                        observed.path = Some(path);
                    }
                }

                let Some(path) = observed.path.as_ref() else {
                    continue;
                };

                let (offset, is_complete) =
                    drain_rollout_file(path, observed.offset, &observed.turn_id).with_context(
                        || format!("failed to drain rollout file {}", path.display()),
                    )?;
                observed.offset = offset;
                if is_complete {
                    completed.push((
                        key.clone(),
                        observed.session_id.clone(),
                        observed.turn_id.clone(),
                    ));
                }
            }

            for key in expired {
                state.observed.remove(&key);
            }
            for (key, _, _) in &completed {
                state.observed.remove(key);
            }
        }

        for (_, session_id, turn_id) in completed {
            self.emit_turn_ended(&session_id, &turn_id);
        }

        Ok(())
    }

    fn emit_turn_ended(&self, session_id: &str, turn_id: &str) {
        let message = json!({
            "jsonrpc": "2.0",
            "id": format!("native-turn-ended:{session_id}:{turn_id}"),
            "method": "turnEnded",
            "params": {
                "session_id": session_id,
                "turn_id": turn_id
            }
        });

        let mut stdout = self.stdout.lock().expect("stdout writer mutex poisoned");
        if let Err(error) = write_frame(&mut *stdout, &message) {
            log(&format!(
                "extension-host: failed to emit turnEnded for session {session_id}: {error}"
            ));
        }
    }
}

fn main() -> Result<()> {
    let socket_dir = socket_dir();
    prepare_socket_dir(&socket_dir)?;
    let socket_path = socket_path(&socket_dir);
    remove_socket_if_present(&socket_path)?;

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod {}", socket_path.display()))?;

    let stdout: SharedChromeWriter = Arc::new(Mutex::new(Box::new(io::stdout())));
    let rollout_tracker = RolloutTracker::new(Arc::clone(&stdout));
    let extension_id = extension_id_from_args();
    let (runtime_config, runtime_config_error) = match load_extension_host_config() {
        Ok(config) => {
            log(&format!(
                "loaded runtime config for {}",
                config.codex_cli_path.display()
            ));
            (Some(config), None)
        }
        Err(error) => {
            let message = format!("failed to load {RUNTIME_CONFIG_FILE}: {error:#}");
            log(&message);
            (None, Some(message))
        }
    };
    let state = Arc::new(Mutex::new(HostState::with_runtime_config(
        stdout,
        rollout_tracker,
        extension_id,
        runtime_config,
        runtime_config_error,
    )));

    log(&format!("listening on {}", socket_path.display()));

    {
        let state = Arc::clone(&state);
        thread::spawn(move || accept_clients(listener, state));
    }

    let result = read_chrome_messages(Arc::clone(&state));
    if let Ok(mut state) = state.lock() {
        state.stop_runtime();
    }
    remove_socket_if_present(&socket_path)?;
    result
}

fn default_runtime_config_schema() -> u64 {
    1
}

fn default_proxy_host() -> String {
    "127.0.0.1".to_string()
}

fn extension_host_config_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os(RUNTIME_CONFIG_PATH_ENV) {
        return Ok(PathBuf::from(path));
    }

    let executable = env::current_exe().context("failed to resolve extension host executable")?;
    let parent = executable
        .parent()
        .context("extension host executable has no parent directory")?;
    Ok(parent.join(RUNTIME_CONFIG_FILE))
}

fn load_extension_host_config() -> Result<ExtensionHostConfig> {
    let path = extension_host_config_path()?;
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut config: ExtensionHostConfig = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    if config.schema_version == 0 {
        bail!("runtime config schemaVersion must be at least 1");
    }
    if config.schema_version > 1 {
        log(&format!(
            "runtime config schemaVersion {} is newer than this host; using known fields",
            config.schema_version
        ));
    }

    validate_executable_path(&config.codex_cli_path, "codexCliPath")?;
    validate_executable_path(&config.node_path, "nodePath")?;
    validate_executable_path(&config.node_repl_path, "nodeReplPath")?;
    if config.proxy_host.trim().is_empty() {
        bail!("proxyHost must not be empty");
    }
    config.proxy_host = config.proxy_host.trim().to_string();
    config.channel = config
        .channel
        .take()
        .map(|channel| channel.trim().to_string())
        .filter(|channel| !channel.is_empty());
    config.node_module_dirs.retain(|path| path.is_dir());

    if config.codex_home.is_none() {
        config.codex_home = env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".codex"))
        });
    }

    if let Some(browser_client_path) = config.browser_client_path.as_ref() {
        if !browser_client_path.is_file() {
            bail!(
                "browserClientPath is not a readable file: {}",
                browser_client_path.display()
            );
        }
        config.trusted_browser_client_sha256s = vec![sha256_file(browser_client_path)?];
    }

    Ok(config)
}

fn validate_executable_path(path: &Path, field: &str) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("{field} does not exist: {}", path.display()))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        bail!("{field} is not executable: {}", path.display());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash {}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

fn socket_dir() -> PathBuf {
    env::var_os(SOCKET_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_DIR))
}

fn sessions_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os(SESSIONS_DIR_ENV).map(PathBuf::from) {
        return Some(path);
    }

    if let Some(path) = env::var_os("CODEX_HOME").map(PathBuf::from) {
        return Some(path.join("sessions"));
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".codex").join("sessions"))
}

fn extension_id_from_args() -> Option<String> {
    env::args().skip(1).find_map(|arg| {
        arg.strip_prefix("chrome-extension://")
            .and_then(|value| value.split('/').next())
            .filter(|value| is_extension_id(value))
            .map(ToString::to_string)
    })
}

fn is_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| matches!(byte, b'a'..=b'p'))
}

fn socket_path(socket_dir: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    socket_dir.join(format!("extension-{}-{nonce}.sock", process::id()))
}

fn prepare_socket_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;

    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if !metadata.file_type().is_dir() {
        bail!(
            "unix socket directory path is not a directory: {}",
            path.display()
        );
    }

    let effective_uid = unsafe { libc::geteuid() };
    if metadata.uid() != effective_uid {
        bail!(
            "unix socket directory is owned by uid {}, expected {}: {}",
            metadata.uid(),
            effective_uid,
            path.display()
        );
    }

    if metadata.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }

    Ok(())
}

fn remove_socket_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn accept_clients(listener: UnixListener, state: SharedState) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                log(&format!("platform accept error: {error}"));
                continue;
            }
        };

        match authorize_peer(&stream) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                log(&format!("peer authorization error: {error}"));
                continue;
            }
        }

        let writer = match stream.try_clone() {
            Ok(stream) => Arc::new(Mutex::new(stream)),
            Err(error) => {
                log(&format!("client socket clone error: {error}"));
                continue;
            }
        };

        let (client_id, evicted_clients) = {
            let mut state = state.lock().expect("host state mutex poisoned");
            state.replace_with_client(writer)
        };
        for (evicted_id, evicted_client) in evicted_clients {
            log(&format!(
                "evicting stale browser client {evicted_id} after a newer client connected"
            ));
            close_client_socket(&evicted_client);
        }

        let state = Arc::clone(&state);
        thread::spawn(move || read_client_messages(state, client_id, stream));
    }
}

fn close_client_socket(client: &Client) {
    match client.writer.lock() {
        Ok(writer) => {
            let _ = writer.shutdown(Shutdown::Both);
        }
        Err(error) => log(&format!("client socket close lock error: {error}")),
    }
}

fn authorize_peer(stream: &UnixStream) -> Result<bool> {
    let credentials = peer_credentials(stream)?;
    let effective_uid = unsafe { libc::geteuid() };

    if credentials.uid != effective_uid {
        log(&format!(
            "rejecting peer pid {} uid {}, expected uid {}",
            credentials.pid, credentials.uid, effective_uid
        ));
        return Ok(false);
    }

    Ok(true)
}

fn peer_credentials(stream: &UnixStream) -> Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };

    if result != 0 {
        return Err(io::Error::last_os_error()).context("failed to read peer credentials");
    }

    Ok(credentials)
}

fn read_chrome_messages(state: SharedState) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    while let Some(message) =
        read_frame(&mut reader).context("extension-host: platform reader error")?
    {
        handle_chrome_message(&state, message);
    }
    Ok(())
}

fn read_client_messages(state: SharedState, client_id: usize, stream: UnixStream) {
    let mut stream = stream;
    loop {
        match read_frame(&mut stream) {
            Ok(Some(message)) => handle_client_message(&state, client_id, message),
            Ok(None) => break,
            Err(error) => {
                log(&format!("client socket read error: {error}"));
                break;
            }
        }
    }

    let mut state = state.lock().expect("host state mutex poisoned");
    state.remove_client(client_id);
}

fn handle_client_message(state: &SharedState, client_id: usize, message: Value) {
    {
        let state = state.lock().expect("host state mutex poisoned");
        if !state.clients.contains_key(&client_id) {
            return;
        }
    }

    if is_response(&message) {
        let Some(id) = message_id_as_str(&message) else {
            return;
        };

        let mut state = state.lock().expect("host state mutex poisoned");
        let Some(pending) = state.pending_client_requests.get(id).cloned() else {
            return;
        };
        if pending.client_id != client_id {
            return;
        }
        state.pending_client_requests.remove(id);

        state.send_chrome(&with_id(message, pending.chrome_request_id));
        return;
    }

    if !is_request(&message) {
        let state = state.lock().expect("host state mutex poisoned");
        if state.clients.contains_key(&client_id) {
            state.send_chrome(&message);
        }
        return;
    }

    {
        let tracker = {
            let state = state.lock().expect("host state mutex poisoned");
            state.rollout_tracker.clone()
        };
        tracker.observe_request(&message);
    }

    if message.get("method").and_then(Value::as_str) == Some("ping") {
        let Some(id) = message.get("id").cloned() else {
            return;
        };
        let state = state.lock().expect("host state mutex poisoned");
        state.send_client(
            client_id,
            &json!({ "jsonrpc": "2.0", "id": id, "result": "pong" }),
        );
        return;
    }

    let Some(client_request_id) = message.get("id").cloned() else {
        return;
    };
    let fallback_extension_info = message.get("method").and_then(Value::as_str) == Some("getInfo");

    let mut state = state.lock().expect("host state mutex poisoned");
    if !state.clients.contains_key(&client_id) {
        return;
    }
    let chrome_id = format!("linux-{}-{}", process::id(), state.next_chrome_id);
    state.next_chrome_id += 1;
    state.pending_chrome_requests.insert(
        chrome_id.clone(),
        PendingChromeRequest {
            client_id,
            client_request_id,
            fallback_extension_info,
        },
    );
    state.send_chrome(&with_id(message, Value::String(chrome_id)));
}

fn is_direct_runtime_method(method: &str) -> bool {
    matches!(
        method,
        "codexRuntime/hello"
            | "codexRuntime/ensure"
            | "codexRuntime/restart"
            | "codexRuntime/tabContextAsset/create"
            | "codexRuntime/tabContextAsset/appendChunk"
            | "codexRuntime/tabContextAsset/finish"
            | "codexRuntime/tabContextAsset/abort"
            | "codexRuntime/tabContextAsset/remove"
    )
}

fn handle_runtime_request(state: &mut HostState, message: &Value) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let result = match method {
        "codexRuntime/hello" => Ok(runtime_hello_result()),
        "codexRuntime/ensure" => ensure_app_server(state, message, false),
        "codexRuntime/restart" => ensure_app_server(state, message, true),
        "codexRuntime/tabContextAsset/create" => create_tab_context_asset(state, message),
        "codexRuntime/tabContextAsset/appendChunk" => {
            append_tab_context_asset_chunk(state, message)
        }
        "codexRuntime/tabContextAsset/finish" => finish_tab_context_asset(state, message),
        "codexRuntime/tabContextAsset/abort" | "codexRuntime/tabContextAsset/remove" => {
            remove_tab_context_asset(state, message)
        }
        _ => Err(RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Unsupported native runtime method: {method}"),
        )),
    };

    match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": error.message,
                "data": { "type": error.error_type }
            }
        }),
    }
}

fn runtime_hello_result() -> Value {
    json!({
        "manifestSchemaVersion": MANIFEST_SCHEMA_VERSION,
        "nativeHostProtocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "supportedProtocolVersions": [NATIVE_HOST_PROTOCOL_VERSION],
        "supportedMethods": [
            "codexRuntime/hello",
            "codexRuntime/ensure",
            "codexRuntime/restart",
            "codexRuntime/tabContextAsset/create",
            "codexRuntime/tabContextAsset/appendChunk",
            "codexRuntime/tabContextAsset/finish",
            "codexRuntime/tabContextAsset/abort",
            "codexRuntime/tabContextAsset/remove"
        ]
    })
}

fn ensure_app_server(
    state: &mut HostState,
    message: &Value,
    restart: bool,
) -> std::result::Result<Value, RuntimeRequestError> {
    let config = state.runtime_config.clone().ok_or_else(|| {
        RuntimeRequestError::new(
            "required_path_missing",
            state.runtime_config_error.clone().unwrap_or_else(|| {
                format!("Missing native host runtime configuration: {RUNTIME_CONFIG_FILE}")
            }),
        )
    })?;
    validate_runtime_constraints(state, &config, message)?;

    if restart {
        if let Some(server) = state.app_server.take() {
            server.stop();
        }
    } else if let Some(server) = state.app_server.as_mut() {
        let proxy_running = server.proxy.is_running();
        match server.child.try_wait() {
            Ok(None) if proxy_running => {
                return Ok(runtime_ensure_result(&config, &server.url));
            }
            Ok(None) => {
                log("managed app-server proxy stopped before ensure");
            }
            Ok(Some(status)) => {
                log(&format!(
                    "managed app-server exited before ensure with status {status}"
                ));
            }
            Err(error) => {
                log(&format!("failed to inspect managed app-server: {error}"));
            }
        }
        if let Some(server) = state.app_server.take() {
            server.stop();
        }
    }

    let server = spawn_app_server(&config, state.extension_id.as_deref()).map_err(|error| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Failed to start Codex app-server: {error:#}"),
        )
    })?;
    let response = runtime_ensure_result(&config, &server.url);
    log(&format!(
        "managed app-server ready at {} (pid {})",
        server.url,
        server.child.id()
    ));
    state.app_server = Some(server);
    Ok(response)
}

fn validate_runtime_constraints(
    state: &HostState,
    config: &ExtensionHostConfig,
    message: &Value,
) -> std::result::Result<(), RuntimeRequestError> {
    let constraints = message
        .get("params")
        .and_then(|params| params.get("constraints"));
    let Some(constraints) = constraints else {
        return Ok(());
    };

    if let Some(required) = constraints
        .get("requiredNativeHostProtocolVersion")
        .and_then(Value::as_u64)
    {
        if required != NATIVE_HOST_PROTOCOL_VERSION {
            return Err(RuntimeRequestError::new(
                "version_mismatch",
                format!(
                    "Native host protocol {required} is required, but this host supports {NATIVE_HOST_PROTOCOL_VERSION}"
                ),
            ));
        }
    }

    if let Some(required) = constraints
        .get("requiredAppServerProtocolVersion")
        .and_then(Value::as_u64)
    {
        if required != APP_SERVER_PROTOCOL_VERSION {
            return Err(RuntimeRequestError::new(
                "version_mismatch",
                format!(
                    "App-server protocol {required} is required, but this host supports {APP_SERVER_PROTOCOL_VERSION}"
                ),
            ));
        }
    }

    if let (Some(expected), Some(actual)) = (
        state.extension_id.as_deref(),
        constraints.get("extensionId").and_then(Value::as_str),
    ) {
        if actual != expected {
            return Err(RuntimeRequestError::new(
                "no_matching_codex_install",
                "The connected extension does not match this native host manifest",
            ));
        }
    }

    if let (Some(expected), Some(actual)) = (
        config.channel.as_deref(),
        constraints
            .get("extensionBuildChannel")
            .and_then(Value::as_str),
    ) {
        if actual != expected {
            return Err(RuntimeRequestError::new(
                "no_matching_codex_install",
                format!("Extension channel {actual} does not match native host channel {expected}"),
            ));
        }
    }

    Ok(())
}

fn runtime_ensure_result(config: &ExtensionHostConfig, url: &str) -> Value {
    let browser_client_path = config
        .browser_client_path
        .as_deref()
        .map(path_to_runtime_string);
    let node_module_dirs = config
        .node_module_dirs
        .iter()
        .map(|path| path_to_runtime_string(path))
        .collect::<Vec<_>>();

    json!({
        "localAppServerUrl": url,
        "runtimeConfig": {
            "browserClientPath": browser_client_path,
            "codexCliPath": path_to_runtime_string(&config.codex_cli_path),
            "nodeModuleDirs": node_module_dirs,
            "nodePath": path_to_runtime_string(&config.node_path),
            "nodeReplPath": path_to_runtime_string(&config.node_repl_path),
            "platform": env::consts::OS,
            "trustedBrowserClientSha256s": config.trusted_browser_client_sha256s
        }
    })
}

fn path_to_runtime_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn stop_app_server_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let process_group = -(child.id() as libc::pid_t);
        // SAFETY: the child creates a dedicated process group before exec.
        unsafe {
            libc::kill(process_group, libc::SIGTERM);
        }
        let deadline = Instant::now() + APP_SERVER_STOP_TIMEOUT;
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        // SAFETY: the same dedicated child process group is still targeted.
        unsafe {
            libc::kill(process_group, libc::SIGKILL);
        }
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn run_app_server_proxy_listener(
    listener: TcpListener,
    backend_address: SocketAddr,
    allowed_origin: Option<String>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer_address)) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if !peer_address.ip().is_loopback() {
                    log(&format!(
                        "rejected non-loopback app-server proxy peer {peer_address}"
                    ));
                    continue;
                }

                let allowed_origin = allowed_origin.clone();
                if let Err(error) = thread::Builder::new()
                    .name("codex-app-server-proxy-connection".to_string())
                    .spawn(move || {
                        if let Err(error) = proxy_app_server_connection(
                            stream,
                            backend_address,
                            allowed_origin.as_deref(),
                        ) {
                            log(&format!("app-server proxy connection failed: {error:#}"));
                        }
                    })
                {
                    log(&format!(
                        "failed to spawn app-server proxy connection: {error}"
                    ));
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(APP_SERVER_PROXY_ACCEPT_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                log(&format!("app-server proxy listener failed: {error}"));
                break;
            }
        }
    }
}

fn proxy_app_server_connection(
    mut client: TcpStream,
    backend_address: SocketAddr,
    allowed_origin: Option<&str>,
) -> Result<()> {
    client
        .set_read_timeout(Some(APP_SERVER_PROXY_HANDSHAKE_TIMEOUT))
        .context("failed to set app-server proxy handshake timeout")?;
    client
        .set_write_timeout(Some(APP_SERVER_PROXY_HANDSHAKE_TIMEOUT))
        .context("failed to set app-server proxy write timeout")?;

    let request = read_proxy_http_headers(&mut client)?;
    let request = match rewrite_proxy_websocket_handshake(&request, allowed_origin) {
        Ok(request) => request,
        Err(error) => {
            write_proxy_http_error(&mut client, "403 Forbidden");
            return Err(error.context("rejected app-server WebSocket handshake"));
        }
    };

    let mut backend =
        match TcpStream::connect_timeout(&backend_address, APP_SERVER_PROXY_HANDSHAKE_TIMEOUT) {
            Ok(backend) => backend,
            Err(error) => {
                write_proxy_http_error(&mut client, "502 Bad Gateway");
                return Err(error).context("failed to connect to Codex app-server backend");
            }
        };
    backend
        .set_write_timeout(Some(APP_SERVER_PROXY_HANDSHAKE_TIMEOUT))
        .context("failed to set app-server backend handshake timeout")?;
    backend
        .write_all(&request)
        .context("failed to forward app-server WebSocket handshake")?;
    backend
        .flush()
        .context("failed to flush app-server WebSocket handshake")?;

    client
        .set_read_timeout(None)
        .context("failed to clear app-server proxy read timeout")?;
    client
        .set_write_timeout(None)
        .context("failed to clear app-server proxy write timeout")?;
    backend
        .set_read_timeout(None)
        .context("failed to clear app-server backend read timeout")?;
    backend
        .set_write_timeout(None)
        .context("failed to clear app-server backend write timeout")?;

    let mut client_reader = client
        .try_clone()
        .context("failed to clone app-server proxy client stream")?;
    let mut backend_writer = backend
        .try_clone()
        .context("failed to clone app-server backend stream")?;
    let upstream = thread::spawn(move || {
        let result = io::copy(&mut client_reader, &mut backend_writer);
        let _ = backend_writer.shutdown(Shutdown::Write);
        result
    });

    let downstream = io::copy(&mut backend, &mut client);
    let _ = client.shutdown(Shutdown::Both);
    let _ = backend.shutdown(Shutdown::Both);
    let upstream = upstream
        .join()
        .map_err(|_| anyhow::anyhow!("app-server proxy upstream thread panicked"))?;
    downstream.context("app-server proxy backend read failed")?;
    upstream.context("app-server proxy client read failed")?;
    Ok(())
}

fn read_proxy_http_headers(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut request = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 4096];

    loop {
        let read = stream
            .read(&mut buffer)
            .context("failed to read app-server proxy HTTP headers")?;
        if read == 0 {
            bail!("connection closed before app-server proxy HTTP headers completed");
        }
        request.extend_from_slice(&buffer[..read]);

        if let Some(header_end) = proxy_http_header_end(&request) {
            if header_end > APP_SERVER_PROXY_MAX_HEADER_BYTES {
                bail!("app-server proxy HTTP headers exceeded 64KiB");
            }
            return Ok(request);
        }
        if request.len() >= APP_SERVER_PROXY_MAX_HEADER_BYTES {
            bail!("app-server proxy HTTP headers exceeded 64KiB");
        }
    }
}

fn rewrite_proxy_websocket_handshake(
    request: &[u8],
    allowed_origin: Option<&str>,
) -> Result<Vec<u8>> {
    let header_end = proxy_http_header_end(request)
        .context("app-server proxy request is missing the HTTP header terminator")?;
    let header_text = std::str::from_utf8(&request[..header_end])
        .context("app-server proxy HTTP headers are not valid UTF-8")?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .context("missing app-server proxy request line")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next();
    let path = request_parts.next();
    let version = request_parts.next();
    if method != Some("GET")
        || !path.is_some_and(|path| path.starts_with('/'))
        || version != Some("HTTP/1.1")
        || request_parts.next().is_some()
    {
        bail!("invalid app-server proxy WebSocket request line");
    }

    let mut rewritten = Vec::with_capacity(request.len());
    rewritten.extend_from_slice(request_line.as_bytes());
    rewritten.extend_from_slice(b"\r\n");

    let mut saw_connection_upgrade = false;
    let mut saw_host = false;
    let mut saw_origin = false;
    let mut saw_websocket_key = false;
    let mut saw_websocket_upgrade = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            bail!("folded app-server proxy HTTP headers are not supported");
        }
        let (name, value) = line
            .split_once(':')
            .context("malformed app-server proxy HTTP header")?;
        if name.is_empty() || name != name.trim() {
            bail!("malformed app-server proxy HTTP header name");
        }
        let value = value.trim();

        if name.eq_ignore_ascii_case("Origin") {
            if saw_origin {
                bail!("duplicate app-server proxy Origin header");
            }
            saw_origin = true;
            let expected = allowed_origin.context(
                "app-server proxy cannot accept a browser Origin without an extension id",
            )?;
            if value != expected {
                bail!("app-server proxy Origin is not the connected extension");
            }
            continue;
        }

        if name.eq_ignore_ascii_case("Connection") {
            saw_connection_upgrade = header_has_token(value, "upgrade");
        } else if name.eq_ignore_ascii_case("Host") {
            saw_host = !value.is_empty();
        } else if name.eq_ignore_ascii_case("Sec-WebSocket-Key") {
            saw_websocket_key = !value.is_empty();
        } else if name.eq_ignore_ascii_case("Upgrade") {
            saw_websocket_upgrade = value.eq_ignore_ascii_case("websocket");
        }

        rewritten.extend_from_slice(line.as_bytes());
        rewritten.extend_from_slice(b"\r\n");
    }

    if !(saw_connection_upgrade && saw_host && saw_websocket_key && saw_websocket_upgrade) {
        bail!("incomplete app-server proxy WebSocket handshake");
    }

    rewritten.extend_from_slice(b"\r\n");
    rewritten.extend_from_slice(&request[header_end..]);
    Ok(rewritten)
}

fn header_has_token(value: &str, expected: &str) -> bool {
    value
        .split(',')
        .any(|token| token.trim().eq_ignore_ascii_case(expected))
}

fn proxy_http_header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn write_proxy_http_error(stream: &mut TcpStream, status: &str) {
    let response = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn spawn_app_server(
    config: &ExtensionHostConfig,
    extension_id: Option<&str>,
) -> Result<AppServerProcess> {
    let proxy_listener = TcpListener::bind((config.proxy_host.as_str(), config.proxy_port))
        .with_context(|| {
            format!(
                "failed to bind Codex app-server proxy on {}:{}",
                config.proxy_host, config.proxy_port
            )
        })?;
    let proxy_address = proxy_listener
        .local_addr()
        .context("failed to inspect Codex app-server proxy address")?;
    if !proxy_address.ip().is_loopback() {
        bail!(
            "refusing to expose the Codex app-server proxy on non-loopback address {proxy_address}"
        );
    }
    let mut proxy_listener = Some(proxy_listener);
    let allowed_origin = extension_id.map(|id| format!("chrome-extension://{id}"));

    let attempts = 3;
    let mut errors = Vec::new();

    for _ in 0..attempts {
        let backend_listener =
            TcpListener::bind((config.proxy_host.as_str(), 0)).with_context(|| {
                format!(
                    "failed to reserve Codex app-server backend on {}",
                    config.proxy_host
                )
            })?;
        let backend_address = backend_listener
            .local_addr()
            .context("failed to inspect reserved Codex app-server backend")?;
        if !backend_address.ip().is_loopback() {
            bail!(
                "refusing to expose the Codex app-server backend on non-loopback address {backend_address}"
            );
        }
        drop(backend_listener);

        let backend_url = format!("ws://{backend_address}");
        let mut command = Command::new(&config.codex_cli_path);
        command
            .arg("app-server")
            .arg("--listen")
            .arg(&backend_url)
            .arg("--analytics-default-enabled")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("CODEX_CLI_PATH", &config.codex_cli_path)
            .env("CODEX_BROWSER_USE_NODE_PATH", &config.node_path)
            .env("NODE_REPL_NODE_PATH", &config.node_path)
            .env("CODEX_NODE_REPL_PATH", &config.node_repl_path)
            .env(
                "CODEX_APP_SERVER_PROXY_HOST",
                proxy_address.ip().to_string(),
            )
            .env(
                "CODEX_APP_SERVER_PROXY_PORT",
                proxy_address.port().to_string(),
            );

        if let Some(browser_client_path) = config.browser_client_path.as_ref() {
            command.env("CODEX_BROWSER_CLIENT_PATH", browser_client_path);
        }
        if let Some(codex_home) = config.codex_home.as_ref() {
            command.env("CODEX_HOME", codex_home);
        }
        if let Some(resources_path) = config.resources_path.as_ref() {
            command.env("CODEX_ELECTRON_RESOURCES_PATH", resources_path);
        }
        if let Some(extension_id) = extension_id {
            command.env("CODEX_EXTENSION_ID", extension_id);
        }
        if let Some(channel) = config.channel.as_ref() {
            command.env("CODEX_EXTENSION_CHANNEL", channel);
        }
        if !config.node_module_dirs.is_empty() {
            command.env(
                "NODE_REPL_NODE_MODULE_DIRS",
                env::join_paths(&config.node_module_dirs)
                    .context("nodeModuleDirs contains an unsupported path")?,
            );
        }

        let parent_pid = process::id() as libc::pid_t;
        // SAFETY: pre_exec only calls async-signal-safe libc functions and constructs no Rust data.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent_pid {
                    libc::raise(libc::SIGTERM);
                }
                Ok(())
            });
        }

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to execute Codex CLI {}",
                config.codex_cli_path.display()
            )
        })?;

        match wait_for_app_server(&mut child, backend_address) {
            Ok(()) => {
                let listener = proxy_listener
                    .take()
                    .context("Codex app-server proxy listener was already consumed")?;
                let proxy = match AppServerProxy::start(
                    listener,
                    backend_address,
                    allowed_origin.clone(),
                ) {
                    Ok(proxy) => proxy,
                    Err(error) => {
                        stop_app_server_child(&mut child);
                        return Err(error);
                    }
                };
                let url = format!("ws://{}", proxy.address);
                return Ok(AppServerProcess { child, url, proxy });
            }
            Err(error) => {
                errors.push(format!("{error:#}"));
                stop_app_server_child(&mut child);
            }
        }
    }

    bail!("{}", errors.join("; "))
}

fn wait_for_app_server(child: &mut Child, address: SocketAddr) -> Result<()> {
    let deadline = Instant::now() + APP_SERVER_START_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .context("failed to inspect Codex app-server process")?
        {
            bail!("Codex app-server exited with status {status}");
        }

        if TcpStream::connect_timeout(&address, APP_SERVER_CONNECT_INTERVAL).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "Codex app-server did not listen on {address} within {} seconds",
                APP_SERVER_START_TIMEOUT.as_secs()
            );
        }
        thread::sleep(APP_SERVER_CONNECT_INTERVAL);
    }
}

fn create_tab_context_asset(
    state: &mut HostState,
    message: &Value,
) -> std::result::Result<Value, RuntimeRequestError> {
    let file_name = runtime_param_string(message, "fileName")?;
    let root = ensure_asset_root(state)?;
    let asset_id = format!("linux-{}-{}", process::id(), state.next_asset_id);
    state.next_asset_id += 1;
    let path = root.join(format!(
        "{asset_id}-{}",
        sanitize_asset_file_name(file_name)
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| {
            RuntimeRequestError::new(
                "app_server_runtime_error",
                format!("Failed to create temporary browser asset: {error}"),
            )
        })?;

    state.assets.insert(
        asset_id.clone(),
        TabContextAsset {
            file: Some(file),
            path: path.clone(),
            written: 0,
        },
    );
    Ok(json!({
        "assetId": asset_id,
        "path": path_to_runtime_string(&path)
    }))
}

fn append_tab_context_asset_chunk(
    state: &mut HostState,
    message: &Value,
) -> std::result::Result<Value, RuntimeRequestError> {
    let asset_id = runtime_param_string(message, "assetId")?;
    let data_base64 = runtime_param_string(message, "dataBase64")?;
    let data = BASE64_STANDARD.decode(data_base64).map_err(|error| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Invalid base64 browser asset chunk: {error}"),
        )
    })?;
    let asset = state.assets.get_mut(asset_id).ok_or_else(|| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Unknown browser asset id: {asset_id}"),
        )
    })?;
    let new_size = asset
        .written
        .checked_add(data.len() as u64)
        .ok_or_else(|| {
            RuntimeRequestError::new("app_server_runtime_error", "Asset is too large")
        })?;
    if new_size > MAX_TAB_CONTEXT_ASSET_BYTES {
        return Err(RuntimeRequestError::new(
            "app_server_runtime_error",
            "Browser asset exceeds the 256 MiB safety limit",
        ));
    }
    let file = asset.file.as_mut().ok_or_else(|| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Browser asset is already finalized: {asset_id}"),
        )
    })?;
    file.write_all(&data).map_err(|error| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Failed to append browser asset: {error}"),
        )
    })?;
    asset.written = new_size;
    Ok(json!({}))
}

fn finish_tab_context_asset(
    state: &mut HostState,
    message: &Value,
) -> std::result::Result<Value, RuntimeRequestError> {
    let asset_id = runtime_param_string(message, "assetId")?;
    let asset = state.assets.get_mut(asset_id).ok_or_else(|| {
        RuntimeRequestError::new(
            "app_server_runtime_error",
            format!("Unknown browser asset id: {asset_id}"),
        )
    })?;
    if let Some(mut file) = asset.file.take() {
        file.flush().map_err(|error| {
            RuntimeRequestError::new(
                "app_server_runtime_error",
                format!("Failed to finalize browser asset: {error}"),
            )
        })?;
    }
    Ok(json!({
        "assetId": asset_id,
        "path": path_to_runtime_string(&asset.path)
    }))
}

fn remove_tab_context_asset(
    state: &mut HostState,
    message: &Value,
) -> std::result::Result<Value, RuntimeRequestError> {
    let asset_id = runtime_param_string(message, "assetId")?;
    if let Some(asset) = state.assets.remove(asset_id) {
        drop(asset.file);
        match fs::remove_file(&asset.path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(RuntimeRequestError::new(
                    "app_server_runtime_error",
                    format!("Failed to remove browser asset: {error}"),
                ));
            }
        }
    }
    Ok(json!({}))
}

fn runtime_param_string<'a>(
    message: &'a Value,
    name: &str,
) -> std::result::Result<&'a str, RuntimeRequestError> {
    message
        .get("params")
        .and_then(|params| params.get(name))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeRequestError::new(
                "app_server_runtime_error",
                format!("Missing native runtime parameter: {name}"),
            )
        })
}

fn ensure_asset_root(state: &mut HostState) -> std::result::Result<PathBuf, RuntimeRequestError> {
    if let Some(root) = state.asset_root.as_ref() {
        return Ok(root.clone());
    }

    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(env::temp_dir);
    for attempt in 0..16_u64 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = base.join(format!(
            "codex-chrome-assets-{}-{nonce}-{attempt}",
            process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&root) {
            Ok(()) => {
                state.asset_root = Some(root.clone());
                return Ok(root);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(RuntimeRequestError::new(
                    "app_server_runtime_error",
                    format!("Failed to create browser asset directory: {error}"),
                ));
            }
        }
    }
    Err(RuntimeRequestError::new(
        "app_server_runtime_error",
        "Failed to allocate a unique browser asset directory",
    ))
}

fn sanitize_asset_file_name(file_name: &str) -> String {
    let base_name = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset.bin");
    let sanitized = base_name
        .chars()
        .take(160)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() || matches!(sanitized.as_str(), "." | "..") {
        "asset.bin".to_string()
    } else {
        sanitized
    }
}

fn handle_chrome_message(state: &SharedState, message: Value) {
    if is_response(&message) {
        let Some(id) = message_id_as_str(&message) else {
            return;
        };

        let mut state = state.lock().expect("host state mutex poisoned");
        let Some(pending) = state.pending_chrome_requests.remove(id) else {
            return;
        };

        // chrome.runtime.getVersion() is available in Chrome/Chromium 143+.
        // Keep forwarding getInfo for browsers that support it, and only
        // synthesize discovery metadata for this older-runtime compatibility
        // failure.
        if pending.fallback_extension_info && is_missing_chrome_runtime_get_version_error(&message)
        {
            state.send_client(
                pending.client_id,
                &extension_info_response(pending.client_request_id, state.extension_id.as_deref()),
            );
            return;
        }

        state.send_client(
            pending.client_id,
            &with_id(message, pending.client_request_id),
        );
        return;
    }

    if is_request(&message) {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if is_direct_runtime_method(method) {
                let mut state = state.lock().expect("host state mutex poisoned");
                let response = handle_runtime_request(&mut state, &message);
                state.send_chrome(&response);
                return;
            }
        }
    }

    if !is_request(&message) {
        let state = state.lock().expect("host state mutex poisoned");
        state.broadcast_clients(&message);
        return;
    }

    let chrome_request_id = message.get("id").cloned().unwrap_or(Value::Null);
    let mut state = state.lock().expect("host state mutex poisoned");
    let client_id = match select_single_client_id(&state.clients) {
        Ok(client_id) => client_id,
        Err(error) => {
            state.send_chrome(&json!({
                "jsonrpc": "2.0",
                "id": chrome_request_id,
                "error": {
                    "code": -32000,
                    "message": error.message()
                }
            }));
            return;
        }
    };

    let client_request_id = format!("chrome-{}-{}", process::id(), state.next_client_request_id);
    state.next_client_request_id += 1;
    state.pending_client_requests.insert(
        client_request_id.clone(),
        PendingClientRequest {
            client_id,
            chrome_request_id,
        },
    );
    state.send_client(
        client_id,
        &with_id(message, Value::String(client_request_id)),
    );
}

fn select_single_client_id(
    clients: &HashMap<usize, Client>,
) -> std::result::Result<usize, ChromeClientRouteError> {
    match clients.len() {
        0 => Err(ChromeClientRouteError::NoClients),
        1 => Ok(*clients.keys().next().expect("one client id")),
        _ => Err(ChromeClientRouteError::MultipleClients),
    }
}

fn remove_pending_requests_for_client(
    pending_chrome_requests: &mut HashMap<String, PendingChromeRequest>,
    pending_client_requests: &mut HashMap<String, PendingClientRequest>,
    client_id: usize,
) {
    pending_chrome_requests.retain(|_, pending| pending.client_id != client_id);
    pending_client_requests.retain(|_, pending| pending.client_id != client_id);
}

fn is_request(message: &Value) -> bool {
    message.get("id").is_some() && message.get("method").and_then(Value::as_str).is_some()
}

fn is_response(message: &Value) -> bool {
    message.get("id").is_some() && message.get("method").and_then(Value::as_str).is_none()
}

fn message_id_as_str(message: &Value) -> Option<&str> {
    message.get("id").and_then(Value::as_str)
}

fn with_id(mut message: Value, id: Value) -> Value {
    if let Value::Object(ref mut object) = message {
        object.insert("id".to_string(), id);
    }
    message
}

fn is_missing_chrome_runtime_get_version_error(message: &Value) -> bool {
    message
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("chrome.runtime.getVersion is not a function"))
}

fn extension_info_response(id: Value, extension_id: Option<&str>) -> Value {
    let mut metadata = serde_json::Map::new();
    if let Some(extension_id) = extension_id {
        metadata.insert(
            "extensionId".to_string(),
            Value::String(extension_id.to_string()),
        );
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "name": "Chrome",
            "version": "unknown",
            "type": "extension",
            "capabilities": {
                "tab": [
                    {
                        "id": "pageAssets",
                        "description": "List assets already observed in the current page state and bundle selected assets into a temporary local artifact."
                    }
                ]
            },
            "metadata": Value::Object(metadata)
        }
    })
}

fn session_turn_from_message(message: &Value) -> Option<(String, String)> {
    let params = message.get("params")?;
    let session_id = non_empty_string(params.get("session_id")?)?;
    let turn_id = non_empty_string(params.get("turn_id")?)?;
    Some((session_id.to_string(), turn_id.to_string()))
}

fn non_empty_string(value: &Value) -> Option<&str> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then_some(value)
}

fn observed_turn_key(session_id: &str, turn_id: &str) -> String {
    format!("{session_id}\n{turn_id}")
}

fn file_len(path: &Path) -> io::Result<u64> {
    Ok(fs::metadata(path)?.len())
}

fn find_rollout_path(root: &Path, session_id: &str) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0_usize)];
    let mut best: Option<(SystemTime, PathBuf)> = None;

    while let Some((dir, depth)) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_dir() {
                if depth < ROLLOUT_SEARCH_MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.contains(session_id)
                || !(file_name.ends_with(".jsonl") || file_name.ends_with(".json"))
            {
                continue;
            }

            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            if best
                .as_ref()
                .is_none_or(|(best_modified, _)| modified > *best_modified)
            {
                best = Some((modified, path));
            }
        }
    }

    best.map(|(_, path)| path)
}

fn drain_rollout_file(path: &Path, offset: u64, turn_id: &str) -> io::Result<(u64, bool)> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(offset.min(len)))?;

    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut is_complete = false;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line_marks_turn_complete(&line, turn_id) {
            is_complete = true;
        }
    }

    Ok((reader.stream_position()?, is_complete))
}

fn line_marks_turn_complete(line: &str, turn_id: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return false;
    };

    let payload = value.get("payload").unwrap_or(&value);
    let payload_type = payload.get("type").and_then(Value::as_str);
    let payload_turn_id = payload.get("turn_id").and_then(Value::as_str);
    if payload_type == Some("task_complete") && payload_turn_id == Some(turn_id) {
        return true;
    }

    let top_level_type = value.get("type").and_then(Value::as_str);
    let kind = value.get("kind").and_then(Value::as_str);
    top_level_type == Some("turn")
        && matches!(kind, Some("end" | "completed" | "complete"))
        && value.get("turn_id").and_then(Value::as_str) == Some(turn_id)
}

fn read_frame(reader: &mut impl Read) -> io::Result<Option<Value>> {
    loop {
        let mut header = [0_u8; 4];
        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        }

        let length = u32::from_ne_bytes(header) as usize;
        let mut body = vec![0_u8; length];
        reader.read_exact(&mut body)?;

        match serde_json::from_slice(&body) {
            Ok(message) => return Ok(Some(message)),
            Err(error) => log(&format!("dropping invalid JSON frame: {error}")),
        }
    }
}

fn write_frame(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    if body.len() > u32::MAX as usize {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "message too large for 4-byte length prefix",
        ));
    }

    writer.write_all(&(body.len() as u32).to_ne_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn log(message: &str) {
    let _ = writeln!(io::stderr(), "[{HOST_NAME}] {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_uses_native_length_prefix() {
        let message = json!({ "jsonrpc": "2.0", "id": "1", "method": "ping" });
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &message).unwrap();

        let length = u32::from_ne_bytes(encoded[..4].try_into().unwrap()) as usize;
        assert_eq!(length, encoded.len() - 4);

        let mut cursor = io::Cursor::new(encoded);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(message));
    }

    #[test]
    fn runtime_hello_uses_protocol_v2_without_a_legacy_client() {
        let (host_state, output) = test_host_state_with_output();
        let state = Arc::new(Mutex::new(host_state));

        handle_chrome_message(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": "hello-1",
                "method": "codexRuntime/hello",
                "params": {
                    "constraints": {
                        "requiredAppServerProtocolVersion": 2,
                        "requiredNativeHostProtocolVersion": 2
                    }
                }
            }),
        );

        let response = read_captured_message(&output);
        assert_eq!(response["id"], "hello-1");
        assert_eq!(response["result"]["manifestSchemaVersion"], 2);
        assert_eq!(response["result"]["nativeHostProtocolVersion"], 2);
        assert_eq!(response["result"]["supportedProtocolVersions"], json!([2]));
        assert!(response.get("error").is_none());
    }

    #[test]
    fn app_server_proxy_strips_only_the_matching_extension_origin() {
        let request = concat!(
            "GET /?clientId=sidepanel-window-42 HTTP/1.1\r\n",
            "Host: 127.0.0.1:12345\r\n",
            "Connection: keep-alive, Upgrade\r\n",
            "Upgrade: websocket\r\n",
            "Sec-WebSocket-Key: SGVsbG9Db2RleDEyMzQ1Ng==\r\n",
            "Sec-WebSocket-Version: 13\r\n",
            "Origin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\n",
            "Sec-WebSocket-Extensions: permessage-deflate\r\n",
            "\r\n",
            "early-bytes"
        );

        let rewritten = rewrite_proxy_websocket_handshake(
            request.as_bytes(),
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop"),
        )
        .unwrap();
        let rewritten_text = String::from_utf8(rewritten).unwrap();

        assert!(rewritten_text.starts_with("GET /?clientId=sidepanel-window-42 HTTP/1.1\r\n"));
        assert!(!rewritten_text.to_ascii_lowercase().contains("\r\norigin:"));
        assert!(rewritten_text.contains("Sec-WebSocket-Extensions: permessage-deflate\r\n"));
        assert!(rewritten_text.ends_with("\r\nearly-bytes"));
    }

    #[test]
    fn app_server_proxy_rejects_a_different_browser_origin() {
        let request = concat!(
            "GET / HTTP/1.1\r\n",
            "Host: 127.0.0.1:12345\r\n",
            "Connection: Upgrade\r\n",
            "Upgrade: websocket\r\n",
            "Sec-WebSocket-Key: SGVsbG9Db2RleDEyMzQ1Ng==\r\n",
            "Origin: https://attacker.example\r\n",
            "\r\n"
        );

        let error = rewrite_proxy_websocket_handshake(
            request.as_bytes(),
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop"),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Origin is not the connected extension"));
    }

    #[test]
    fn app_server_proxy_forwards_a_browser_websocket_handshake() {
        let backend_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend_address = backend_listener.local_addr().unwrap();
        let (request_sender, request_receiver) = std::sync::mpsc::channel();
        let backend_thread = thread::spawn(move || {
            let (mut stream, _) = backend_listener.accept().unwrap();
            let request = read_proxy_http_headers(&mut stream).unwrap();
            request_sender.send(request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
                )
                .unwrap();
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut proxy = AppServerProxy::start(
            proxy_listener,
            backend_address,
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop".to_string()),
        )
        .unwrap();
        let mut client = TcpStream::connect(proxy.address).unwrap();
        client
            .write_all(
                concat!(
                    "GET /?clientId=sidepanel-window-42 HTTP/1.1\r\n",
                    "Host: 127.0.0.1:12345\r\n",
                    "Connection: Upgrade\r\n",
                    "Upgrade: websocket\r\n",
                    "Sec-WebSocket-Key: SGVsbG9Db2RleDEyMzQ1Ng==\r\n",
                    "Origin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\n",
                    "\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();

        let backend_request = String::from_utf8(request_receiver.recv().unwrap()).unwrap();
        assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(backend_request.starts_with("GET /?clientId=sidepanel-window-42 HTTP/1.1\r\n"));
        assert!(!backend_request.to_ascii_lowercase().contains("\r\norigin:"));

        proxy.stop();
        backend_thread.join().unwrap();
    }

    #[test]
    fn tab_context_asset_lifecycle_is_confined_to_private_runtime_directory() {
        let mut state = test_host_state();
        let created = create_tab_context_asset(
            &mut state,
            &json!({ "params": { "fileName": "../../report?.txt" } }),
        )
        .unwrap();
        let asset_id = created["assetId"].as_str().unwrap().to_string();
        let path = PathBuf::from(created["path"].as_str().unwrap());
        assert_eq!(path.parent(), state.asset_root.as_deref());
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("report_.txt"));

        append_tab_context_asset_chunk(
            &mut state,
            &json!({
                "params": {
                    "assetId": asset_id,
                    "dataBase64": BASE64_STANDARD.encode(b"hello")
                }
            }),
        )
        .unwrap();
        let finished =
            finish_tab_context_asset(&mut state, &json!({ "params": { "assetId": asset_id } }))
                .unwrap();
        assert_eq!(finished["path"], created["path"]);
        assert_eq!(fs::read(&path).unwrap(), b"hello");

        remove_tab_context_asset(&mut state, &json!({ "params": { "assetId": asset_id } }))
            .unwrap();
        assert!(!path.exists());
        state.stop_runtime();
    }

    #[test]
    fn id_replacement_preserves_other_fields() {
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "getTabs" });
        assert_eq!(
            with_id(message, Value::String("linux-1-1".to_string())),
            json!({ "jsonrpc": "2.0", "id": "linux-1-1", "method": "getTabs" })
        );
    }

    #[test]
    fn extracts_session_turn_from_browser_request() {
        let message = json!({
            "jsonrpc": "2.0",
            "id": "request-1",
            "method": "getTabs",
            "params": {
                "session_id": "session-1",
                "turn_id": "turn-1"
            }
        });

        assert_eq!(
            session_turn_from_message(&message),
            Some(("session-1".to_string(), "turn-1".to_string()))
        );
    }

    #[test]
    fn recognizes_task_complete_rollout_line() {
        let line = r#"{"timestamp":"2026-05-09T12:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#;
        assert!(line_marks_turn_complete(line, "turn-1"));
        assert!(!line_marks_turn_complete(line, "turn-2"));
    }

    #[test]
    fn finds_nested_rollout_path_by_session_id() {
        let root = unique_test_dir("codex-rollout-path");
        let nested = root.join("2026").join("05").join("09");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("rollout-2026-05-09T12-00-00-session-1.jsonl");
        fs::write(&path, "{}\n").unwrap();

        assert_eq!(find_rollout_path(&root, "session-1"), Some(path));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn drains_rollout_file_from_offset() {
        let root = unique_test_dir("codex-rollout-drain");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("rollout-session-1.jsonl");
        fs::write(
            &path,
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"other\"}}\n",
        )
        .unwrap();
        let offset = file_len(&path).unwrap();

        let complete =
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#;
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "ignored\n{complete}"
        )
        .unwrap();
        let (new_offset, is_complete) = drain_rollout_file(&path, offset, "turn-1").unwrap();

        assert!(new_offset >= offset);
        assert!(is_complete);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn late_discovered_rollout_file_scans_existing_content() {
        let root = unique_test_dir("codex-rollout-late");
        let nested = root.join("2026").join("05").join("09");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("rollout-session-1.jsonl");
        let complete =
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#;
        writeln!(File::create(&path).unwrap(), "{complete}").unwrap();

        let discovered = find_rollout_path(&root, "session-1").unwrap();
        let (_, is_complete) = drain_rollout_file(&discovered, 0, "turn-1").unwrap();

        assert!(is_complete);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_chrome_request_routing_without_exactly_one_client() {
        let clients = HashMap::new();
        assert_eq!(
            select_single_client_id(&clients),
            Err(ChromeClientRouteError::NoClients)
        );

        let mut clients = HashMap::new();
        clients.insert(7, test_client());
        assert_eq!(select_single_client_id(&clients), Ok(7));

        clients.insert(8, test_client());
        assert_eq!(
            select_single_client_id(&clients),
            Err(ChromeClientRouteError::MultipleClients)
        );
    }

    #[test]
    fn replacing_browser_client_evicts_stale_clients_and_pending_requests() {
        let mut state = test_host_state();

        let (first_client_id, evicted_clients) =
            state.replace_with_client(test_client().writer.clone());
        assert!(evicted_clients.is_empty());
        assert!(state.clients.contains_key(&first_client_id));

        state.pending_chrome_requests.insert(
            "chrome-request".to_string(),
            PendingChromeRequest {
                client_id: first_client_id,
                client_request_id: json!("client-request-1"),
                fallback_extension_info: false,
            },
        );
        state.pending_client_requests.insert(
            "client-request".to_string(),
            PendingClientRequest {
                client_id: first_client_id,
                chrome_request_id: json!("chrome-request-1"),
            },
        );

        let (second_client_id, evicted_clients) =
            state.replace_with_client(test_client().writer.clone());

        assert_ne!(first_client_id, second_client_id);
        assert_eq!(evicted_clients.len(), 1);
        assert_eq!(evicted_clients[0].0, first_client_id);
        assert!(!state.clients.contains_key(&first_client_id));
        assert!(state.clients.contains_key(&second_client_id));
        assert!(state.pending_chrome_requests.is_empty());
        assert!(state.pending_client_requests.is_empty());
    }

    #[test]
    fn evicted_client_requests_are_ignored() {
        let state = Arc::new(Mutex::new(test_host_state()));

        handle_client_message(
            &state,
            99,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "getTabs" }),
        );

        let state = state.lock().unwrap();
        assert!(state.pending_chrome_requests.is_empty());
        assert_eq!(state.next_chrome_id, 1);
    }

    #[test]
    fn forwards_client_raw_cdp_call_requests_to_chrome_without_filtering() {
        let (mut host_state, output) = test_host_state_with_output();
        host_state.clients.insert(1, test_client());
        let state = Arc::new(Mutex::new(host_state));
        let request = json!({
            "jsonrpc": "2.0",
            "id": "client-cdp-call-1",
            "method": "tab_cdp_call",
            "params": {
                "browser_id": "browser-1",
                "tab_id": "42",
                "method": "Runtime.evaluate",
                "params": {
                    "expression": "document.title",
                    "returnByValue": true
                },
                "target": {
                    "target_id": "target-1"
                },
                "timeout_ms": 5000
            }
        });

        handle_client_message(&state, 1, request.clone());

        let chrome_id = format!("linux-{}-1", process::id());
        let forwarded = read_captured_message(&output);
        assert_eq!(forwarded["id"], chrome_id);
        assert_eq!(forwarded["method"], "tab_cdp_call");
        assert_eq!(forwarded["params"], request["params"]);

        let state = state.lock().unwrap();
        let pending = state.pending_chrome_requests.get(&chrome_id).unwrap();
        assert_eq!(pending.client_id, 1);
        assert_eq!(pending.client_request_id, json!("client-cdp-call-1"));
        assert!(!pending.fallback_extension_info);
    }

    #[test]
    fn forwards_client_raw_cdp_event_requests_to_chrome_without_filtering() {
        let (mut host_state, output) = test_host_state_with_output();
        host_state.clients.insert(1, test_client());
        let state = Arc::new(Mutex::new(host_state));
        let request = json!({
            "jsonrpc": "2.0",
            "id": "client-cdp-events-1",
            "method": "tab_cdp_events",
            "params": {
                "after_sequence": 7,
                "browser_id": "browser-1",
                "limit": 25,
                "methods": ["Runtime.consoleAPICalled", "Target.attachedToTarget"],
                "tab_id": "42",
                "target": {
                    "session_id": "session-1"
                },
                "timeout_ms": 500
            }
        });

        handle_client_message(&state, 1, request.clone());

        let forwarded = read_captured_message(&output);
        assert_eq!(forwarded["id"], format!("linux-{}-1", process::id()));
        assert_eq!(forwarded["method"], "tab_cdp_events");
        assert_eq!(forwarded["params"], request["params"]);
    }

    #[test]
    fn forwards_chrome_raw_cdp_responses_to_the_requesting_client() {
        let (client_writer, mut client_reader) = UnixStream::pair().unwrap();
        let mut state = test_host_state();
        state.clients.insert(
            1,
            Client {
                writer: Arc::new(Mutex::new(client_writer)),
            },
        );
        state.pending_chrome_requests.insert(
            "linux-1-1".to_string(),
            PendingChromeRequest {
                client_id: 1,
                client_request_id: json!("client-cdp-call-1"),
                fallback_extension_info: false,
            },
        );
        let state = Arc::new(Mutex::new(state));

        handle_chrome_message(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": "linux-1-1",
                "result": {
                    "result": {
                        "type": "string",
                        "value": "Codex"
                    }
                }
            }),
        );

        let message = read_frame(&mut client_reader).unwrap().unwrap();
        assert_eq!(message["id"], "client-cdp-call-1");
        assert_eq!(message["result"]["result"]["value"], "Codex");
        assert!(state.lock().unwrap().pending_chrome_requests.is_empty());
    }

    #[test]
    fn get_info_falls_back_when_runtime_get_version_is_missing() {
        let (client_writer, mut client_reader) = UnixStream::pair().unwrap();
        let mut state = test_host_state();
        state.clients.insert(
            1,
            Client {
                writer: Arc::new(Mutex::new(client_writer)),
            },
        );
        state.pending_chrome_requests.insert(
            "linux-1-1".to_string(),
            PendingChromeRequest {
                client_id: 1,
                client_request_id: json!("info-1"),
                fallback_extension_info: true,
            },
        );
        state.extension_id = Some("abcdefghijklmnopabcdefghijklmnop".to_string());
        let state = Arc::new(Mutex::new(state));

        handle_chrome_message(
            &state,
            json!({
                "jsonrpc": "2.0",
                "id": "linux-1-1",
                "error": {
                    "code": 1,
                    "message": "chrome.runtime.getVersion is not a function"
                }
            }),
        );

        let message = read_frame(&mut client_reader).unwrap().unwrap();
        assert_eq!(message["id"], "info-1");
        assert_eq!(message["result"]["type"], "extension");
        assert_eq!(message["result"]["version"], "unknown");
        assert_eq!(
            message["result"]["metadata"]["extensionId"],
            "abcdefghijklmnopabcdefghijklmnop"
        );
        assert!(state.lock().unwrap().pending_chrome_requests.is_empty());
    }

    #[test]
    fn disconnect_cleanup_removes_pending_state_for_client() {
        let mut pending_chrome = HashMap::from([
            (
                "keep".to_string(),
                PendingChromeRequest {
                    client_id: 1,
                    client_request_id: json!("chrome-request-1"),
                    fallback_extension_info: false,
                },
            ),
            (
                "drop".to_string(),
                PendingChromeRequest {
                    client_id: 2,
                    client_request_id: json!("chrome-request-2"),
                    fallback_extension_info: false,
                },
            ),
        ]);
        let mut pending_client = HashMap::from([
            (
                "keep".to_string(),
                PendingClientRequest {
                    client_id: 1,
                    chrome_request_id: json!("client-request-1"),
                },
            ),
            (
                "drop".to_string(),
                PendingClientRequest {
                    client_id: 2,
                    chrome_request_id: json!("client-request-2"),
                },
            ),
        ]);

        remove_pending_requests_for_client(&mut pending_chrome, &mut pending_client, 2);

        assert!(pending_chrome.contains_key("keep"));
        assert!(!pending_chrome.contains_key("drop"));
        assert!(pending_client.contains_key("keep"));
        assert!(!pending_client.contains_key("drop"));
    }

    fn test_client() -> Client {
        let (stream, _peer) = UnixStream::pair().unwrap();
        Client {
            writer: Arc::new(Mutex::new(stream)),
        }
    }

    fn test_host_state() -> HostState {
        let stdout: SharedChromeWriter = Arc::new(Mutex::new(Box::new(io::stdout())));
        HostState::new(
            Arc::clone(&stdout),
            RolloutTracker {
                inner: Arc::new(Mutex::new(RolloutTrackerState {
                    observed: HashMap::new(),
                })),
                stdout,
                sessions_root: None,
            },
            Some("abcdefghijklmnopabcdefghijklmnop".to_string()),
        )
    }

    fn test_host_state_with_output() -> (HostState, Arc<Mutex<Vec<u8>>>) {
        let output = Arc::new(Mutex::new(Vec::new()));
        let stdout: SharedChromeWriter = Arc::new(Mutex::new(Box::new(CaptureWriter {
            output: Arc::clone(&output),
        })));
        let state = HostState::new(
            Arc::clone(&stdout),
            RolloutTracker {
                inner: Arc::new(Mutex::new(RolloutTrackerState {
                    observed: HashMap::new(),
                })),
                stdout,
                sessions_root: None,
            },
            Some("abcdefghijklmnopabcdefghijklmnop".to_string()),
        );
        (state, output)
    }

    fn read_captured_message(output: &Arc<Mutex<Vec<u8>>>) -> Value {
        let data = output.lock().unwrap().clone();
        let mut cursor = io::Cursor::new(data);
        read_frame(&mut cursor).unwrap().unwrap()
    }

    struct CaptureWriter {
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for CaptureWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.output.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{}-{nonce}", process::id()))
    }
}
