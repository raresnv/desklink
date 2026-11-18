'use strict';
const request = document.querySelector('#demo-request');
const approval = document.querySelector('#demo-approval');
const connected = document.querySelector('#demo-connected');
const status = document.querySelector('#demo-status');
const input = document.querySelector('#demo-id');
function resetDemo(message) {
  approval.hidden = true; connected.hidden = true; request.disabled = false;
  status.textContent = message; request.focus({preventScroll:true});
}
request.addEventListener('click', () => {
  const digits = input.value.replace(/[\s-]/g, '');
  if (!/^[1-9]\d{11}$/.test(digits)) {
    status.textContent = 'Enter a sample 12-digit ID to try the demo.';
    input.focus({preventScroll:true}); return;
  }
  approval.hidden = false; request.disabled = true;
  status.textContent = 'The host sees a request and decides whether to allow it.';
  document.querySelector('#demo-allow').focus({preventScroll:true});
});
document.querySelector('#demo-allow').addEventListener('click', () => {
  approval.hidden = true; connected.hidden = false;
  status.textContent = 'Only approval starts a session. Either person can disconnect.';
  document.querySelector('#demo-disconnect').focus({preventScroll:true});
});
document.querySelector('#demo-decline').addEventListener('click', () => resetDemo('Request declined. No screen was shared.'));
document.querySelector('#demo-disconnect').addEventListener('click', () => resetDemo('Disconnected. Access has ended. Try again whenever you like.'));
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && (!approval.hidden || !connected.hidden)) resetDemo('Demo closed. No connection to a real computer was made.');
});
document.querySelectorAll('a[href^="#"]').forEach(link => link.addEventListener('click', () => {
  const target = document.getElementById(link.hash.slice(1));
  if (target instanceof HTMLDetailsElement) target.open = true;
}));
fetch('/release.json').then(response => {
  if (!response.ok) throw new Error('Release manifest unavailable');
  return response.json();
}).then(release => {
  for (const item of [...release.packages, ...(release.additional_packages || [])]) {
    const anchor = document.querySelector(`[data-package="${item.id || item.platform}"]`);
    const size = document.querySelector(`[data-package-size="${item.id || item.platform}"]`);
    if (anchor && item.url.startsWith('/downloads/')) anchor.href = item.url;
    if (size) size.textContent = `${(item.bytes / 1048576).toFixed(1)} MB · ${item.format}`;
  }
  const agent = navigator.userAgent;
  const platform = /Macintosh|Mac OS X/.test(agent) && !/iPhone|iPad|iPod/.test(agent) ? 'macos' : /Windows/.test(agent) ? 'windows' : null;
  const item = release.packages.find(item => item.platform === platform);
  if (item) {
    const button = document.querySelector('.platform-download');
    button.href = item.url; button.setAttribute('download', '');
    button.querySelector('.download-label').textContent = platform === 'macos' ? 'Download for Mac' : 'Download for Windows';
    document.querySelector('#platform-note').textContent = `${release.version} preview · ${platform === 'macos' ? 'Apple Silicon · macOS 12.3+' : 'Windows x64'}`;
  }
}).catch(() => { /* Static, versioned download links remain fully usable. */ });
