function initMagPage() {
  const copyButtons = document.querySelectorAll('[data-copy]');

  for (const button of copyButtons) {
    button.addEventListener('click', async () => {
      const label = button.querySelector('.copy-label');
      const original = label?.textContent ?? 'Copy';

      try {
        await navigator.clipboard.writeText(button.dataset.copy ?? '');
        button.classList.add('is-copied');
        if (label) label.textContent = 'Copied';
        window.setTimeout(() => {
          button.classList.remove('is-copied');
          if (label) label.textContent = original;
        }, 1800);
      } catch {
        if (label) label.textContent = 'Select command';
      }
    });
  }
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initMagPage, { once: true });
} else {
  initMagPage();
}
