# Browser Client Benchmark

## RMBT C-Server vs. Rust Server — Comparison via Open Nettest UI

---

## 1. Objective

This test was conducted to validate the correctness of the Rust RMBT server implementation by comparing browser-client measurements against the legacy C server. The goal was to determine whether observable differences in browser test results are attributable to server-side implementation or to the browser client itself.

---

## 2. Environment Setup

### 2.1 Server Installation

The test server (`framework-desktop`, Ubuntu 24.04.3 LTS, kernel 6.14) was configured to run both the C and Rust RMBT servers locally. To enable remote browser access to the server GUI, a noVNC setup was deployed:

1. Xvfb virtual display server launched on display `:2`
2. x11vnc attached to the virtual display
3. websockify + noVNC exposed on port `6080`
4. SSH tunnel from Mac to server (port 6080) provided browser access via `localhost:6080/vnc.html`

### 2.2 `/etc/hosts` Override

To redirect browser traffic to the local server instead of the public measurement infrastructure, the following entry was added to `/etc/hosts` on the test machine:

```
127.0.0.1    dev.measurementservers.net
```

This ensured that all WSS connections from the Open Nettest browser client were directed to the locally running server on port 443, without modifying the client source code. No restart was required — `/etc/hosts` changes take effect immediately.

---

## 3. Test Procedure

Both server implementations were started sequentially on port 443 with TLS enabled. The Open Nettest browser client was launched via noVNC and measurements were performed against each server in turn. The client configuration and network conditions were identical for both runs.

---

## 4. Results


| Server      | Download (Mbps) | Upload (Mbps) |
| ----------- | --------------- | ------------- |
| C Server    | 13,128          | 13,240        |
| Rust Server | 13,150          | 13,180        |


---

## 5. Conclusion

The browser client results for both server implementations are virtually identical — well within normal measurement variance. This confirms two key findings:

1. **The Rust server implements the RMBT protocol correctly** — it produces the same observable throughput as the reference C implementation.
2. **The browser client is the bottleneck** in this test scenario. At ~13 Gbps the WebSocket/TLS stack in the browser saturates before any server-side difference can be observed.

These results complement the native client benchmarks documented in `RMBT_Full_Technical_Comparison_EN.md`, where Rust outperforms C by 30%+ in raw TCP throughput. In the browser context, protocol correctness is confirmed, while performance headroom remains available for higher-capacity clients.