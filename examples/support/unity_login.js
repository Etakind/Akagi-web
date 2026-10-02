// Evaluated only in the official page. Returns fixed codes, never input values.
(kind, value) => {
  if (location.href !== 'https://game.maj-soul.com/1/' || window.top !== window)
    return 'ORIGIN_REJECTED';
  if ([...document.querySelectorAll('iframe')].some(e => /captcha|challenge/i.test(e.src)))
    return 'USER_VERIFICATION_REQUIRED';
  if (!window.unityInstance || !document.querySelector('#unity-canvas'))
    return 'WAIT_UNITY_CLIENT';
  const e = document.activeElement;
  if (!(e instanceof HTMLInputElement) || e.disabled || e.readOnly ||
      !e.isConnected || !e.getClientRects().length ||
      getComputedStyle(e).visibility !== 'visible' || e.value !== '')
    return 'WAIT_FOCUSED_EMPTY_FIELD';
  // Do not accept search/chat/verification-code/new-password controls.
  const label = (e.getAttribute('aria-label') || e.placeholder || '').trim();
  if (kind === 'email') {
    if (!['text', 'email'].includes(e.type) ||
        !/^(请输入|請輸入)?(邮箱|郵箱|电子邮箱|電子郵箱|邮箱地址|郵箱地址|邮箱账号|邮箱帐号|账号|帐号|帳號|email|email address|電郵\/賬號登錄)(…|\.{3})?$/i.test(label))
      return 'FIELD_REJECTED';
  } else if (kind === 'password') {
    if (e.type !== 'password' ||
        (label && !/^(请输入|請輸入)?(密码|密碼|password)(…|\.{3})?$/i.test(label)))
      return 'FIELD_REJECTED';
  } else return 'FIELD_REJECTED';
  if (e.form && new URL(e.form.action, location.href).origin !== location.origin)
    return 'ORIGIN_REJECTED';
  if (value === undefined) return 'FIELD_READY';
  if (typeof value !== 'string' || (e.maxLength >= 0 && value.length > e.maxLength))
    return 'FIELD_CAPACITY_REJECTED';
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
  set.call(e, value);
  e.dispatchEvent(new Event('input', {bubbles: true}));
  e.dispatchEvent(new Event('change', {bubbles: true}));
  return 'FIELD_FILLED';
}
