/** Independently authored normal fixtures; never derive expectations from output. */
export const formats = ['mihomo_yaml', 'uri_list', 'base64', 'singbox_json', 'xray_json', 'v2ray_json', 'shadowrocket'];
export const families = ['SOCKS5', 'HTTP', 'HTTPS', 'socks5h'];
export const profiles = ['mihomo', 'sing-box', 'xray', 'v2ray', 'shadowrocket', 'uri_list', 'json'];
export type ProxySample = {
  family: string; name: string; host: string; port: number; username?: string; password?: string;
  dns?: string; tls: boolean; sni?: string; verify?: boolean;
};

export function samples(format: string): ProxySample[] {
  const result: ProxySample[] = [];
  for (const [familyIndex, family] of families.entries()) {
    // These native outbound formats have no per-node remote DNS URI intent.
    if (family === 'socks5h' && ['singbox_json', 'xray_json', 'v2ray_json'].includes(format)) continue;
    for (const [hostIndex, host] of ['192.0.2.17', '2001:db8::17', 'a.proxy.example', 'aaaa.proxy.example', 'dual.proxy.example'].entries()) {
      for (const auth of [false, true]) {
        const name = `${family}-${hostIndex}-${auth ? 'auth' : 'none'}`;
        result.push({ family, name, host, port: 18080 + familyIndex * 100 + hostIndex * 2 + Number(auth),
          username: auth ? 'synthetic+user@示例' : undefined, password: auth ? 'TEST:p@ss/#%+&' : undefined,
          dns: family === 'SOCKS5' ? 'unspecified' : family === 'socks5h' ? 'remote' : undefined,
          tls: family === 'HTTPS', sni: family === 'HTTPS' ? 'certificate.example' : undefined,
          verify: family === 'HTTPS' ? false : undefined });
      }
    }
  }
  return result;
}

export function asUri(s: ProxySample) {
  const scheme = s.family === 'SOCKS5' ? 'socks' : s.family.toLowerCase();
  const auth = s.username ? `${encodeURIComponent(s.username)}:${encodeURIComponent(s.password!)}@` : '';
  const host = s.host.includes(':') ? `[${s.host}]` : s.host;
  return `${scheme}://${auth}${host}:${s.port}${s.tls ? '?sni=certificate.example&skip-cert-verify=0' : ''}#${encodeURIComponent(s.name)}`;
}

export function content(format: string, rows: ProxySample[], flowJson = false) {
  if (['uri_list', 'base64', 'shadowrocket'].includes(format)) {
    const text = rows.map(asUri).join('\n');
    return format === 'uri_list' ? text : Buffer.from(text).toString('base64');
  }
  if (format === 'mihomo_yaml') {
    const proxies = rows.map(s => ({
    name: s.name, type: s.family === 'SOCKS5' ? 'socks5' : s.family === 'socks5h' ? 'socks5h' : 'http',
    server: s.host, port: s.port, username: s.username, password: s.password,
    ...(s.tls ? { tls: true, sni: s.sni, 'skip-cert-verify': s.verify } : {}),
    }));
    return flowJson ? JSON.stringify({ proxies }) : `proxies:\n${proxies.map(proxy => `  - ${JSON.stringify(proxy)}`).join('\n')}`;
  }
  if (format === 'singbox_json') return JSON.stringify({ outbounds: rows.map(s => ({
    type: s.family === 'SOCKS5' ? 'socks' : 'http', tag: s.name, server: s.host, server_port: s.port,
    ...(s.family === 'SOCKS5' ? { version: '5' } : {}), username: s.username, password: s.password,
    ...(s.tls ? { tls: { enabled: true, server_name: s.sni, insecure: s.verify } } : {}),
  })) });
  return JSON.stringify({ outbounds: rows.map(s => ({
    protocol: s.family === 'SOCKS5' ? 'socks' : 'http', tag: s.name,
    settings: { servers: [{ address: s.host, port: s.port, ...(s.username ? { users: [{ user: s.username, pass: s.password }] } : {}) }] },
    ...(s.tls ? { streamSettings: { network: 'tcp', security: 'tls', tlsSettings: { serverName: s.sni, allowInsecure: s.verify } } } : {}),
  })) });
}

export function rowId(format: string, family: string, entry: string) {
  return `NEW-${String(families.indexOf(family) * 14 + formats.indexOf(format) * 2 + (entry === 'UI' ? 1 : 2)).padStart(3, '0')}`;
}
