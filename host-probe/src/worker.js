// Module worker for ticket 09. Its whole job is to prove the host can serve a
// .wasm module to a worker with MIME application/wasm, that instantiateStreaming
// accepts it from the network (no manual bytes shuffle), and that the exports run.
self.onmessage = async ({ data }) => {
  const url = data?.url;
  const t0 = performance.now();
  try {
    const res = await fetch(url);
    const headers = {
      status: res.status,
      contentType: res.headers.get("content-type") || "",
      cacheControl: res.headers.get("cache-control") || "",
      contentEncoding: res.headers.get("content-encoding") || "",
      etag: res.headers.get("etag") || "",
    };
    const bytes = (await res.clone().arrayBuffer()).byteLength;
    const instantiated = await WebAssembly.instantiateStreaming(res, {});
    const instantiateMs = performance.now() - t0;
    const ex = instantiated.instance.exports;
    const t1 = performance.now();
    const bits = ex.probe_bits(5_000_000, 7) >>> 0;
    const computeMs = performance.now() - t1;
    self.postMessage({
      ok: true,
      url,
      headers,
      bytes,
      instantiateMs,
      computeMs,
      abiVersion: ex.abi_version(),
      sum: ex.add(2, 40),
      bits,
    });
  } catch (err) {
    self.postMessage({ ok: false, url, error: String((err && err.stack) || err) });
  }
};
