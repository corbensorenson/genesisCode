use super::*;

struct Body {
    remaining: usize,
    consumed: usize,
    max_read: usize,
    interrupted: bool,
}
impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.interrupted {
            self.interrupted = false;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = buffer.len().min(self.remaining).min(self.max_read);
        buffer[..n].fill(b'x');
        self.remaining -= n;
        self.consumed += n;
        Ok(n)
    }
}

#[test]
fn streaming_read_consumes_at_most_limit_plus_one() {
    for max in [0, 1, 1024, 8192, 8193, 20000] {
        for extra in [0, 1, 1000000] {
            for max_read in [1, 29, usize::MAX] {
                let mut body = Body {
                    remaining: max + extra,
                    consumed: 0,
                    max_read,
                    interrupted: true,
                };
                let result = read_bytes_limited("test", &mut body, Some(max));
                if extra == 0 {
                    let bytes = result.unwrap();
                    assert_eq!(bytes, vec![b'x'; max]);
                    assert!(bytes.capacity() <= max);
                    assert_eq!(body.consumed, max);
                } else {
                    assert!(
                        matches!(result, Err(RegistryError::Protocol(message)) if message.starts_with("resource-limit:"))
                    );
                    assert_eq!(body.consumed, max + 1);
                }
            }
        }
    }
}

#[test]
fn maximum_limit_does_not_overflow_and_io_errors_stay_errors() {
    assert_eq!(
        read_bytes_limited("test", &mut &b"ok"[..], Some(usize::MAX)).unwrap(),
        b"ok"
    );
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
    assert!(matches!(
        read_bytes_limited("test", &mut Broken, Some(0)),
        Err(RegistryError::Http(_))
    ));
}

#[test]
fn a_body_growing_after_its_initial_size_is_still_bounded() {
    struct Growing {
        first: bool,
        consumed: usize,
    }
    impl Read for Growing {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let count = if self.first {
                self.first = false;
                1
            } else {
                out.len()
            };
            out[..count].fill(b'x');
            self.consumed += count;
            Ok(count)
        }
    }
    let mut body = Growing {
        first: true,
        consumed: 0,
    };
    assert!(read_bytes_limited("test", &mut body, Some(8192)).is_err());
    assert_eq!(body.consumed, 8193);
}
