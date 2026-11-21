const SOURCES = [['chat', 'Chat'], ['web', 'Web'], ['scheduler', '自動実行']];

export async function mountSessionFilterControls(container, invoke) {
  container.innerHTML = `
    <fieldset class="instance-switches session-filter-switches">
      <legend>表示するセッション</legend>
      ${SOURCES.map(([value, label]) => `<label class="xangi-profile-check"><input type="checkbox" value="${value}" disabled />${label}</label>`).join('')}
    </fieldset>
    <p class="picker-hint" role="status">設定を読み込み中…</p>`;
  const inputs = [...container.querySelectorAll('input')];
  const status = container.querySelector('[role="status"]');
  const apply = (filters) => inputs.forEach((input) => { input.checked = filters.includes(input.value); });
  let saved;
  try {
    saved = await invoke('get_session_filters');
    apply(saved);
    inputs.forEach((input) => { input.disabled = false; });
    status.textContent = '全接続先に適用します。常駐ペットは表示を続けます。変更はすぐに保存されます。';
  } catch (error) {
    status.textContent = `設定を読み込めませんでした: ${error}`;
    return;
  }
  container.addEventListener('change', async (event) => {
    if (!inputs.includes(event.target)) return;
    const filters = inputs.filter((input) => input.checked).map((input) => input.value);
    inputs.forEach((input) => { input.disabled = true; });
    status.textContent = '保存中…';
    try {
      saved = await invoke('set_session_filters', { filters });
      apply(saved);
      status.textContent = saved.length ? '保存しました。表示中のセッションペットにも反映します。' : '保存しました。セッションペットをすべて隠します。';
    } catch (error) {
      apply(saved);
      status.textContent = `保存できませんでした: ${error}`;
    } finally {
      inputs.forEach((input) => { input.disabled = false; });
    }
  });
}
