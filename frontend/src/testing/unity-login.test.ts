// This exercises the exact script embedded by the Rust acceptance tool, with
// fake secrets in a DOM. No requests or account-file access are involved.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
const source = readFileSync(resolve('../examples/support/unity_login.js'), 'utf8');
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';

function fixture(url = 'https://game.maj-soul.com/1/', type = 'text', label = '请输入邮箱') {
  const dom = new JSDOM('<canvas id="unity-canvas"></canvas><input>', { url, runScripts: 'outside-only' });
  const w = dom.window;
  Object.assign(w, { unityInstance: {} });
  const input = w.document.querySelector('input')!;
  input.type = type;
  input.placeholder = label;
  input.style.visibility = 'visible';
  input.getClientRects = () => [{ width: 100 }] as unknown as DOMRectList;
  input.focus();
  const run = w.eval(`(${source})`) as (kind: string, value?: string) => string;
  return { dom, w, input, run };
}
describe('Unity login field boundary', () => {
  it('fills only the focused labeled email, emits events, and never returns its value', () => {
    const { input, run } = fixture();
    const events: string[] = [];
    for (const name of ['input', 'change']) input.addEventListener(name, () => events.push(name));
    expect(run('email')).toBe('FIELD_READY');
    expect(input.value).toBe('');
    expect(run('email', 'synthetic@example.test')).toBe('FIELD_FILLED');
    expect(input.value).toBe('synthetic@example.test');
    expect(events).toEqual(['input', 'change']);
  });
  it('accepts the observed official Unity email label', () => {
    const { input, run } = fixture(undefined, 'text', '電郵/賬號登錄');
    expect(run('email')).toBe('FIELD_READY');
    expect(input.value).toBe('');
  });
  it('preserves password whitespace and punctuation exactly', () => {
    const { input, run } = fixture(undefined, 'password', '请输入密码');
    const fake = '  synthetic-"\\<>密碼\t ';
    expect(run('password', fake)).toBe('FIELD_FILLED');
    expect(input.value).toBe(fake);
  });
  it.each(['https://game.maj-soul.com.evil.test/1/', 'http://game.maj-soul.com/1/', 'https://game.maj-soul.com/other/'])('rejects nonapproved page %s', url => {
    const { input, run } = fixture(url);
    expect(run('email', 'fake')).toBe('ORIGIN_REJECTED');
    expect(input.value).toBe('');
  });
  it.each(['聊天', '验证码', '请输入新密码', '请输入确认密码'])('rejects unrelated label %s', label => {
    const { input, run } = fixture(undefined, 'password', label);
    expect(run('password', 'fake')).toBe('FIELD_REJECTED');
    expect(input.value).toBe('');
  });
  it('never puts a password in a text field', () => {
    const { input, run } = fixture(undefined, 'text', '请输入密码');
    expect(run('password', 'fake')).toBe('FIELD_REJECTED');
    expect(input.value).toBe('');
  });
  it('refuses populated, hidden, and readonly controls', () => {
    const { input, run } = fixture();
    input.value = 'already typed';
    expect(run('email', 'fake')).toBe('WAIT_FOCUSED_EMPTY_FIELD');
    expect(input.value).toBe('already typed');
    input.value = '';
    input.style.visibility = 'hidden';
    expect(run('email', 'fake')).toBe('WAIT_FOCUSED_EMPTY_FIELD');
    input.style.visibility = 'visible'; input.readOnly = true;
    expect(run('email', 'fake')).toBe('WAIT_FOCUSED_EMPTY_FIELD');
  });
  it('does not truncate a password or fill during a challenge', () => {
    const { w, input, run } = fixture(undefined, 'password', '密码');
    input.maxLength = 2;
    expect(run('password', 'synthetic')).toBe('FIELD_CAPACITY_REJECTED');
    const frame = w.document.createElement('iframe'); frame.src = 'https://example.test/captcha';
    w.document.body.appendChild(frame);
    expect(run('password', 'x')).toBe('USER_VERIFICATION_REQUIRED');
    expect(input.value).toBe('');
  });
  it('rejects an external form action', () => {
    const { w, input, run } = fixture();
    const form = w.document.createElement('form'); form.action = 'https://example.test/';
    w.document.body.appendChild(form); form.appendChild(input); input.focus();
    expect(run('email', 'fake')).toBe('ORIGIN_REJECTED');
    expect(input.value).toBe('');
  });
});
