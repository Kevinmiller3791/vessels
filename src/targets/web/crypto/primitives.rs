use crate::{crypto::primitives::{SymmetricKey, NonceProvider, Nonce}, executor};
use failure::Error;
use futures::{lazy, sync::mpsc::channel, Future, Sink, Stream};
use stdweb::{
    unstable::TryInto,
    web::{ArrayBuffer, TypedArray},
};
use std::sync::Mutex;

type CryptoKey = stdweb::Value;

pub(crate) struct AESKey<T: NonceProvider> {
    key: CryptoKey,
    nonce_provider: Mutex<T>
}

impl<T: NonceProvider> SymmetricKey<T> for AESKey<T> {
    fn encrypt(&self, data: &'_ [u8]) -> Box<dyn Future<Item = Vec<u8>, Error = Error> + Send>
    where
        Self: Sized,
    {
        let (sender, receiver) = channel(0);
        let iv = self.nonce_provider.lock().unwrap().next_encrypt();
        let js_iv: TypedArray<u8> = iv.as_ref().clone().as_ref().into();
        let data: TypedArray<u8> = data.to_owned().as_slice().into();
        js! {
            let iv = @{js_iv};
            window.crypto.subtle.encrypt({
                name: "AES-GCM",
                iv: iv,
            }, @{&self.key}, @{data}).then(
                @{move |data: ArrayBuffer| {
                    let s = sender.clone();
                    let mut data: Vec<u8> = data.into();
                    iv.after_encrypt(&mut data);
                    executor::spawn(sender.clone().send(data).then(|_| Ok(())));
                }}
            );
        };
        Box::new(
            receiver
                .take(1)
                .into_future().and_then(|data| Ok(data.0.unwrap()))
                .map_err(|_| failure::err_msg("temp err")),
        )
    }
    fn decrypt(&self, data: &'_ [u8]) -> Box<dyn Future<Item = Vec<u8>, Error = Error> + Send>
    where
        Self: Sized,
    {
        let (sender, receiver) = channel(0);
        let mut data = data.to_owned();
        let iv: TypedArray<u8> = self.nonce_provider.lock().unwrap().next_decrypt(&mut data).as_ref().into();
        let data: TypedArray<u8> = data.as_slice().into();
        js! {
            window.crypto.subtle.decrypt({
                name: "AES-GCM",
                iv: @{iv},
            }, @{&self.key}, @{data}).then((decrypted) => {
                @{move |data: ArrayBuffer| {
                    let s = sender.clone();
                    executor::spawn(sender.clone().send(data).then(|_| Ok(())));
                }}(decrypted);
            });
        };
        Box::new(
            receiver
                .take(1)
                .into_future()
                .and_then(|i| Ok(i.0.unwrap().into()))
                .map_err(|_| failure::err_msg("temp err")),
        )
    }
    fn as_bytes(&self) -> Box<dyn Future<Item = [u8; 16], Error = Error>> {
        let (sender, receiver) = channel(0);
        js! {
            window.crypto.subtle.exportKey("raw", @{&self.key}).then((key) => {
                @{move |data: ArrayBuffer| {
                    let s = sender.clone();
                    let data: Vec<u8> = data.into();
                    let mut a: [u8; 16] = Default::default();
                    a.copy_from_slice(data.as_slice());
                    executor::spawn(sender.clone().send(a).then(|_| Ok(())));
                }}(key);
            });
        };
        Box::new(
            receiver
                .take(1)
                .into_future()
                .and_then(|i| Ok(i.0.unwrap().into()))
                .map_err(|_| failure::err_msg("temp err")),
        )
    }
}

impl<T: NonceProvider + 'static> AESKey<T> {
    pub(crate) fn new() -> impl Future<Item = Box<dyn SymmetricKey<T> + 'static>, Error = Error> {
        let (sender, receiver) = channel(0);
        js! {
            window.crypto.subtle.generateKey({
                name: "AES-GCM",
                length: 128
            }, true, ["encrypt", "decrypt"]).then(@{move |key: CryptoKey| {
                executor::spawn(sender.clone().send(key).then(|_| Ok(())));
            }}).catch((err) => {
                console.log(err);
            });
        };
        receiver
            .take(1)
            .into_future()
            .and_then(|item| {
                let key: Box<dyn SymmetricKey<T>> = Box::new(AESKey {
                    key: item.0.unwrap(),
                    nonce_provider: Mutex::new(T::new())
                });
                Ok(key)
            })
            .map_err(|_| failure::err_msg("temp err"))
    }
    pub(crate) fn from_bytes(data: [u8; 16]) -> impl Future<Item = Box<dyn SymmetricKey<T> + 'static>, Error = Error> {
        let (sender, receiver) = channel(0);
        let data: TypedArray<u8> = data.as_ref().into();
        js! {
            window.crypto.subtle.importKey("raw", @{data}, "AES-GCM", true, ["encrypt", "decrypt"]).then(@{move |key: CryptoKey| {
                executor::spawn(sender.clone().send(key).then(|_| Ok(())));
            }}).catch((err) => {
                console.log(err);
            });
        };
        receiver
            .take(1)
            .into_future()
            .and_then(|item| {
                let key: Box<dyn SymmetricKey<T>> = Box::new(AESKey {
                    key: item.0.unwrap(),
                    nonce_provider: Mutex::new(T::new()),
                });
                Ok(key)
            })
            .map_err(|_| failure::err_msg("temp err"))
    }
}
