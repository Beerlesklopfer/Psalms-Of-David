// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server over a real socket, and the first administrator.

use std::net::SocketAddr;
use std::time::Duration;

use david_server::app::{Config, init_admin, run};
use david_server::{Error, Store};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<david_server::Result<()>>,
    _dir: tempfile::TempDir,
}

async fn start(prepare: impl AsyncFnOnce(&std::path::Path)) -> Running {
    let dir = tempfile::TempDir::new().unwrap();
    prepare(dir.path()).await;
    let config = Config {
        data_dir: dir.path().to_owned(),
        library: None,
        allow_import: false,
        announce: false,
        mcp: false,
        log: None,
        tls: Default::default(),
        features: Default::default(),
        also_listen: Vec::new(),
        listen: "127.0.0.1:0".parse().unwrap(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let (stop, stop_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(run(
        config,
        async move {
            let _ = stop_rx.await;
        },
        move |addr| {
            let _ = ready_tx.send(addr);
        },
    ));
    let addr = tokio::time::timeout(Duration::from_secs(20), ready_rx).await.expect("server did not start").unwrap();
    Running { addr, stop, task, _dir: dir }
}

impl Running {
    async fn stop(self) -> david_server::Result<()> {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(10), self.task).await.expect("server did not stop").unwrap()
    }
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

/// One HTTP/1.1 request over a fresh connection.
async fn http(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if method == "POST" {
        req.push_str(&format!("Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Reply { status, head: head.to_ascii_lowercase(), body: body.to_owned() }
}

#[tokio::test]
async fn the_interface_is_served_over_a_socket_and_stops_cleanly() {
    let server = start(async |dir| {
        init_admin(dir, "Boss", "483920").await.unwrap();
    })
    .await;
    assert!(server.addr.ip().is_loopback());

    let page = http(server.addr, "GET", "/admin/login", &[], "").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("Sign in"));
    assert!(page.head.contains("content-security-policy") && page.head.contains("x-frame-options: deny"));

    // Signed out, the overview leads to the sign-in page.
    let overview = http(server.addr, "GET", "/admin/", &[], "").await;
    assert_eq!(overview.status, 303);
    assert!(overview.head.contains("location: /admin/login"));

    // Sign in for real and use the cookie.
    let login_cookie = page
        .head
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: "))
        .and_then(|c| c.split(';').next())
        .unwrap()
        .to_owned();
    let marker = "name=\"csrf\" value=\"";
    let from = page.body.find(marker).unwrap() + marker.len();
    let token = page.body[from..].split('"').next().unwrap();
    let login = http(server.addr, "POST", "/admin/login", &[("Cookie", &login_cookie)], &format!("name=Boss&pin=483920&csrf={token}")).await;
    assert_eq!(login.status, 303, "{}", login.body);
    let session = login
        .head
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: david_session="))
        .expect("a session cookie");
    let cookie = format!("david_session={}", session.split(';').next().unwrap());
    let users = http(server.addr, "GET", "/admin/users", &[("Cookie", &cookie)], "").await;
    assert_eq!(users.status, 200);
    assert!(users.body.contains("<strong>Boss</strong>") && users.body.contains(">Administrator</td>"));

    // The static files come from the program itself.
    let js = http(server.addr, "GET", "/admin/static/htmx.min.js", &[], "").await;
    assert_eq!(js.status, 200);
    assert!(js.body.len() > 40_000);

    server.stop().await.expect("a clean shutdown");
}

#[tokio::test]
async fn an_unknown_address_is_a_404_not_a_crash() {
    let server = start(async |_| {}).await;
    assert_eq!(http(server.addr, "GET", "/nothing/here", &[], "").await.status, 404);
    assert_eq!(http(server.addr, "GET", "/admin/rpc/user.create", &[], "").await.status, 405, "procedures are POST only");
    // A broken body must not take the server down.
    let r = http(server.addr, "POST", "/admin/login", &[], "%%%not a form").await;
    assert!(r.status >= 400 && r.status < 500, "{}", r.status);
    assert_eq!(http(server.addr, "GET", "/admin/login", &[], "").await.status, 200, "still alive");
    server.stop().await.unwrap();
}

#[tokio::test]
async fn the_server_starts_without_any_user_and_the_import_stays_locked() {
    // A library that holds a book is not read while the import is locked.
    let server = start(async |dir| {
        let lib = dir.join("library/Author/Book");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("a.mp3"), b"not audio").unwrap();
    })
    .await;
    assert_eq!(http(server.addr, "GET", "/admin/login", &[], "").await.status, 200);
    server.stop().await.unwrap();
}

#[tokio::test]
async fn the_first_administrator_is_created_once() {
    let dir = tempfile::TempDir::new().unwrap();
    init_admin(dir.path(), "Boss", "483920").await.unwrap();

    // A second call is refused, whatever name and PIN it brings.
    assert!(matches!(init_admin(dir.path(), "Eve", "111111").await, Err(Error::AdminExists)));
    // A bad PIN is refused before anything is created.
    let other = tempfile::TempDir::new().unwrap();
    assert!(matches!(init_admin(other.path(), "Boss", "12").await, Err(Error::InvalidPin)));
    assert!(!other.path().join("david.db").exists(), "nothing was created, not even the database");
    init_admin(other.path(), "Boss", "123456").await.expect("the refused attempt must not block a good one");

    let store = Store::open(&dir.path().join("david.db")).await.unwrap();
    let names: Vec<_> = store.list_profiles().await.unwrap().into_iter().map(|p| (p.name, p.role)).collect();
    assert_eq!(names, [("Boss".to_owned(), "admin".to_owned())]);
    assert!(matches!(store.verify_login("Boss", "483920").await.unwrap(), david_server::auth::Login::Ok { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn a_data_folder_made_by_the_server_is_closed_to_other_users() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::TempDir::new().unwrap();
    let data = parent.path().join("data");
    init_admin(&data, "Boss", "483920").await.unwrap();
    let mode = std::fs::metadata(&data).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "mode {mode:o}");

    // A folder that already exists is not touched: its owner chose its rights.
    let existing = parent.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755)).unwrap();
    init_admin(&existing, "Boss", "483920").await.unwrap();
    assert_eq!(std::fs::metadata(&existing).unwrap().permissions().mode() & 0o777, 0o755);
}
