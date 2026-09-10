# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\plugin\src\sidecar\manager.rs")
t = p.read_text(encoding="utf-8")
for x in ["SidecarStatus", "Option<Child>", "Option<u16>",
          "Option<oneshot::Sender<()>>", "Option<JoinHandle<()>>"]:
    t = t.replace("std::sync::MutexGuard<%s>" % x, "std::sync::MutexGuard<'_, %s>" % x)
p.write_text(t, encoding="utf-8")
print("lifetimes fixed")
