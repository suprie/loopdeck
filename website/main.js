const steps = [
  { title: 'Discover what is already there.', copy: 'Selasar scans your local workspace for repositories, reads the signals already in them, and gives you one calm view of the work in progress.', command: 'Scan workspace', result: '12 repositories found' },
  { title: 'Make the context durable.', copy: 'Import a project and Selasar creates a small, human-readable memory layer inside the repo — ready to be reviewed, committed, and shared.', command: 'Import project', result: '.loopdeck/ memory created' },
  { title: 'Keep the work moving.', copy: 'Open a project and start a loop from the desktop app. Selasar begins a fresh agent conversation from the next unchecked step and updates project memory as it goes.', command: 'Start loop  ·  desktop app', result: 'agent loop started' },
];

const stepButtons = [...document.querySelectorAll('.loop-step')];
const number = document.querySelector('#step-number');
const title = document.querySelector('#step-title');
const copy = document.querySelector('#step-copy');
const command = document.querySelector('#terminal-command');
const result = document.querySelector('#terminal-result');

stepButtons.forEach((button) => {
  button.addEventListener('click', () => {
    const index = Number(button.dataset.step);
    const step = steps[index];
    stepButtons.forEach((item, itemIndex) => {
      const active = itemIndex === index;
      item.classList.toggle('active', active);
      item.setAttribute('aria-selected', String(active));
    });
    number.textContent = String(index + 1).padStart(2, '0');
    title.textContent = step.title;
    copy.textContent = step.copy;
    command.textContent = step.command;
    result.textContent = step.result;
  });
});

const menuButton = document.querySelector('.menu-toggle');
const nav = document.querySelector('.site-nav');
menuButton.addEventListener('click', () => {
  const open = nav.classList.toggle('open');
  menuButton.setAttribute('aria-expanded', String(open));
});

nav.querySelectorAll('a').forEach((link) => link.addEventListener('click', () => {
  nav.classList.remove('open');
  menuButton.setAttribute('aria-expanded', 'false');
}));
