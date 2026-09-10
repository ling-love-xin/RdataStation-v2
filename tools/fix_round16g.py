# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\known_hosts.rs")
t = p.read_text(encoding="utf-8")

old = """    #[test]
    fn test_parse_non_hashed_entry() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();

        let content = format!("example.com {}\\n", key_b64);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("example.com", 22, &test_key));"""
new = """    #[test]
    fn test_parse_non_hashed_entry() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();
        eprintln!("DIAG key_b64={:?}", key_b64);

        let content = format!("example.com {}\\n", key_b64);
        eprintln!("DIAG content={:?}", content);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);
        eprintln!("DIAG entries keys: {:?}", hosts.entries.keys().collect::<Vec<_>>());
        eprintln!("DIAG entry count: {:?}", hosts.entries.values().map(|v| v.len()).collect::<Vec<_>>());

        assert!(hosts.verify("example.com", 22, &test_key));"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("diag added")
