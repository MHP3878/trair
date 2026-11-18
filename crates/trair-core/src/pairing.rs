// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Pairing a second device (milestone M2, decisions of 18.11.2026: the
//! new device gets the full account key; a short code plus PAKE).
//!
//! The device that is already unlocked (the host) shows a code such as
//! `7-anker-birne-kerze`: a channel number for the server and three words
//! from a list of 256, so 24 random bits. The new device (the guest) types
//! the code (or scans it as a QR code). Both run SPAKE2 with the code as
//! the password. Only with the same code do both get the same key; a
//! relay in the middle learns nothing about the code and gets exactly one
//! guess, because the host stops after the first failed attempt.
//!
//! Messages, all passed through an untrusted relay (later the server):
//!
//! 1. host -> guest: host hello (SPAKE2)
//! 2. guest -> host: guest hello (SPAKE2)
//! 3. guest -> host: offer, encrypted: the guest's new device id and key
//! 4. host -> guest: bundle, encrypted: account key, the new signed device
//!    list (now with the guest), profile
//!
//! The two encryption keys are derived from the SPAKE2 key and a hash of
//! both hello messages, one key per direction. The host's session works
//! once and for ten minutes.

use std::time::{Duration, Instant};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use openmls_basic_credential::SignatureKeyPair;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use zeroize::Zeroizing;

use crate::account::Account;
use crate::devices::{DeviceEntry, DeviceIdentity, SignedDeviceList};
use crate::error::{CoreError, Result, internal};
use crate::vault::{JoinedAccount, Vault};

/// How long a pairing code works.
pub const LIFETIME: Duration = Duration::from_secs(10 * 60);

const CONTEXT: &[u8] = b"trair/pairing/v1";
const NONCE_LEN: usize = 24;

/// The 256 code words. Short, common, no umlauts, none is the start of
/// another.
pub const WORDS: [&str; 256] = [
    "abend", "acker", "adler", "affe", "ahorn", "akte", "alarm", "alpen", "ameise", "amsel",
    "anker", "anruf", "apfel", "april", "arena", "atlas", "auge", "august", "bach", "backe",
    "bagger", "bahn", "ball", "band", "bank", "bart", "baum", "becher", "beere", "berg", "besen",
    "bett", "biber", "biene", "bild", "birke", "birne", "blatt", "blech", "blitz", "blume",
    "boden", "bogen", "bohne", "boot", "brief", "brille", "brot", "bruder", "buch", "bude", "burg",
    "busch", "butter", "dach", "dackel", "dampf", "daumen", "decke", "degen", "deich", "delfin",
    "dino", "dose", "drache", "draht", "duft", "dusche", "ecke", "efeu", "eiche", "eimer", "elch",
    "elster", "emil", "engel", "ente", "erbse", "erde", "esel", "eule", "fabel", "faden", "fahne",
    "falke", "farbe", "fass", "feder", "feld", "fels", "ferien", "fisch", "flagge", "floh", "flur",
    "fluss", "fohlen", "forst", "frosch", "fuchs", "funke", "gabel", "gans", "garten", "geige",
    "geld", "gipfel", "glas", "gold", "gras", "gurke", "hafen", "hagel", "hahn", "hand", "harfe",
    "hase", "haus", "hecht", "hecke", "heft", "held", "helm", "hemd", "herz", "honig", "horn",
    "hose", "hund", "igel", "insel", "jacke", "juli", "juni", "kabel", "kahn", "kamel", "kamin",
    "kanu", "karte", "kasse", "katze", "kegel", "keks", "kerze", "kies", "kino", "kiste", "klee",
    "knopf", "koch", "komet", "korb", "kran", "krone", "kugel", "lachs", "laden", "lager", "lampe",
    "land", "laub", "licht", "lilie", "linde", "loch", "luchs", "luft", "markt", "maus", "meer",
    "meise", "milch", "minze", "mond", "moos", "motor", "mund", "nadel", "nagel", "nase", "nebel",
    "nelke", "nest", "netz", "nuss", "oase", "ofen", "onkel", "orgel", "otter", "paket", "palme",
    "panda", "park", "pfeil", "pferd", "pilz", "pirat", "pony", "post", "puma", "rabe", "radio",
    "rasen", "raupe", "regen", "reis", "rinde", "ring", "robbe", "rock", "rose", "rubin", "saal",
    "salz", "sand", "schaf", "schal", "segel", "sieb", "socke", "sofa", "sonne", "stern", "stift",
    "stuhl", "tafel", "tanne", "taube", "teich", "tiger", "tinte", "tisch", "topf", "tulpe",
    "turm", "ufer", "uhr", "ulme", "vase", "vogel", "wagen", "wald", "wanne", "welle", "wiese",
    "wind", "wolke", "wolle", "wurm", "zange", "zebra", "zelt", "ziege", "zwerg",
];

/// A pairing code: channel number plus three words.
#[derive(Clone, PartialEq, Eq)]
pub struct Code {
    channel: u16,
    words: [u8; 3],
}

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The words are the secret; only the channel is shown.
        write!(f, "Code({}-…)", self.channel)
    }
}

impl Code {
    /// A fresh random code on `channel`.
    pub fn random(channel: u16) -> Result<Self> {
        let mut words = [0u8; 3];
        getrandom::fill(&mut words).map_err(internal)?;
        Ok(Code { channel, words })
    }

    /// Read a typed code. Case, spaces and dashes are forgiven:
    /// `7 Anker birne-KERZE` is the same as `7-anker-birne-kerze`.
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<String> = text
            .split(|c: char| c == '-' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .map(str::to_lowercase)
            .collect();
        let bad = || CoreError::InvalidName("pairing code: number and three words".into());
        let [channel, a, b, c] = parts.as_slice() else {
            return Err(bad());
        };
        let channel: u16 = channel.parse().map_err(|_| bad())?;
        let index = |w: &str| {
            WORDS
                .iter()
                .position(|x| *x == w)
                .and_then(|i| u8::try_from(i).ok())
                .ok_or_else(bad)
        };
        Ok(Code {
            channel,
            words: [index(a)?, index(b)?, index(c)?],
        })
    }

    /// The channel number (where the server keeps the messages).
    pub fn channel(&self) -> u16 {
        self.channel
    }

    /// The code as people read it.
    pub fn text(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            self.channel,
            WORDS[usize::from(self.words[0])],
            WORDS[usize::from(self.words[1])],
            WORDS[usize::from(self.words[2])]
        )
    }

    fn password(&self) -> Password {
        let mut p = CONTEXT.to_vec();
        p.push(0);
        p.extend_from_slice(self.text().as_bytes());
        Password::new(p)
    }
}

/// Keys of one pairing run, one per direction.
struct Keys {
    host_to_guest: Zeroizing<[u8; 32]>,
    guest_to_host: Zeroizing<[u8; 32]>,
    transcript: [u8; 32],
}

impl Keys {
    fn derive(shared: &[u8], host_hello: &[u8], guest_hello: &[u8]) -> Keys {
        let mut t = Sha256::new();
        t.update(CONTEXT);
        for m in [host_hello, guest_hello] {
            t.update((m.len() as u64).to_le_bytes());
            t.update(m);
        }
        let transcript: [u8; 32] = t.finalize().into();
        let sub = |label: &[u8]| {
            let mut h = Sha256::new();
            h.update(CONTEXT);
            h.update(label);
            h.update(shared);
            h.update(transcript);
            Zeroizing::new(h.finalize().into())
        };
        Keys {
            host_to_guest: sub(b"/host-to-guest"),
            guest_to_host: sub(b"/guest-to-host"),
            transcript,
        }
    }
}

fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(internal)?;
    let sealed = XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .map_err(internal)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN {
        return Err(CoreError::Rejected("pairing message too short".into()));
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&sealed[..NONCE_LEN]),
            Payload {
                msg: &sealed[NONCE_LEN..],
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| CoreError::Rejected("pairing failed: wrong code or changed message".into()))
}

#[derive(Serialize, Deserialize)]
struct Offer {
    v: u32,
    device: DeviceEntry,
}

#[derive(Serialize)]
struct BundleRef<'a> {
    v: u32,
    user: &'a str,
    account: &'a SignatureKeyPair,
    list: &'a SignedDeviceList,
    profile: &'a Value,
}

#[derive(Deserialize)]
struct Bundle {
    v: u32,
    user: String,
    account: SignatureKeyPair,
    list: SignedDeviceList,
    profile: Value,
}

/// The unlocked device that shows the code.
pub struct PairingHost {
    spake: Option<Spake2<Ed25519Group>>,
    hello: Vec<u8>,
    started: Instant,
}

impl PairingHost {
    /// Start pairing on `channel`. Returns the session, the code to show
    /// and the host hello for the relay.
    pub fn start(channel: u16, now: Instant) -> Result<(PairingHost, Code, Vec<u8>)> {
        let code = Code::random(channel)?;
        let (spake, hello) =
            Spake2::<Ed25519Group>::start_symmetric(&code.password(), &Identity::new(CONTEXT));
        Ok((
            PairingHost {
                spake: Some(spake),
                hello: hello.clone(),
                started: now,
            },
            code,
            hello,
        ))
    }

    /// Take the guest's hello and offer. On success the guest's device is
    /// added to the account's device list in `vault` (seal and store the
    /// vault afterwards) and the bundle for the guest is returned. Works
    /// once: after any failure the code is used up.
    pub fn finish(
        &mut self,
        guest_hello: &[u8],
        offer: &[u8],
        vault: &mut Vault,
        now: Instant,
    ) -> Result<Vec<u8>> {
        let spake = self
            .spake
            .take()
            .ok_or_else(|| CoreError::Rejected("pairing code already used".into()))?;
        if now.saturating_duration_since(self.started) > LIFETIME {
            return Err(CoreError::Rejected("pairing code expired".into()));
        }
        let shared = Zeroizing::new(
            spake
                .finish(guest_hello)
                .map_err(|_| CoreError::Rejected("pairing failed: bad guest hello".into()))?,
        );
        let keys = Keys::derive(&shared, &self.hello, guest_hello);
        let plain = open(&keys.guest_to_host, &keys.transcript, offer)?;
        let offer: Offer = serde_json::from_slice(&plain)
            .map_err(|_| CoreError::Rejected("pairing offer is not readable".into()))?;
        if offer.v != 1 {
            return Err(CoreError::Rejected("unknown pairing offer".into()));
        }
        let list = vault
            .device_list()
            .with_device(vault.account(), offer.device)?;
        vault
            .set_device_list(list.clone())
            .map_err(|e| CoreError::Rejected(format!("{e}")))?;
        let bundle = BundleRef {
            v: 1,
            user: vault.account().user(),
            account: vault.account().keys(),
            list: &list,
            profile: vault.profile(),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&bundle).map_err(internal)?);
        seal(&keys.host_to_guest, &keys.transcript, &plain)
    }
}

/// The new device that typed the code.
pub struct PairingGuest {
    spake: Option<Spake2<Ed25519Group>>,
    hello: Vec<u8>,
    device: Option<DeviceIdentity>,
    keys: Option<Keys>,
}

impl PairingGuest {
    /// Start with a typed code. Returns the session and the guest hello.
    pub fn start(code: &Code) -> Result<(PairingGuest, Vec<u8>)> {
        let (spake, hello) =
            Spake2::<Ed25519Group>::start_symmetric(&code.password(), &Identity::new(CONTEXT));
        Ok((
            PairingGuest {
                spake: Some(spake),
                hello: hello.clone(),
                device: None,
                keys: None,
            },
            hello,
        ))
    }

    /// Take the host hello; returns the encrypted offer with this
    /// device's new id and key.
    pub fn offer(&mut self, host_hello: &[u8]) -> Result<Vec<u8>> {
        let spake = self
            .spake
            .take()
            .ok_or_else(|| CoreError::Rejected("pairing already in progress".into()))?;
        let shared = Zeroizing::new(
            spake
                .finish(host_hello)
                .map_err(|_| CoreError::Rejected("pairing failed: bad host hello".into()))?,
        );
        let keys = Keys::derive(&shared, host_hello, &self.hello);
        let device = DeviceIdentity::generate()?;
        let offer = serde_json::to_vec(&Offer {
            v: 1,
            device: device.entry(),
        })
        .map_err(internal)?;
        let sealed = seal(&keys.guest_to_host, &keys.transcript, &offer)?;
        self.device = Some(device);
        self.keys = Some(keys);
        Ok(sealed)
    }

    /// Take the host's bundle. Checks that the account signed a device list
    /// that contains this device with its key.
    pub fn finish(mut self, bundle: &[u8]) -> Result<JoinedAccount> {
        let (Some(keys), Some(device)) = (self.keys.take(), self.device.take()) else {
            return Err(CoreError::Rejected("pairing not started".into()));
        };
        let plain = open(&keys.host_to_guest, &keys.transcript, bundle)?;
        let b: Bundle = serde_json::from_slice(&plain)
            .map_err(|_| CoreError::Rejected("pairing bundle is not readable".into()))?;
        if b.v != 1 {
            return Err(CoreError::Rejected("unknown pairing bundle".into()));
        }
        let account = Account::from_keys(&b.user, b.account)?;
        let list = b.list.verify(account.public_key(), None)?;
        if list.user != account.user() || !list.devices.contains(&device.entry()) {
            return Err(CoreError::Rejected(
                "this device is not on the account's list".into(),
            ));
        }
        Ok(JoinedAccount {
            account,
            device,
            list: b.list,
            profile: b.profile,
        })
    }
}
