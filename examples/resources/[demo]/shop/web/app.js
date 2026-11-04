const resource = GetParentResourceName();

function post(name, data) {
  return fetch(`https://${resource}/${name}`, { method: 'POST', body: JSON.stringify(data ?? {}) });
}

window.addEventListener('message', (event) => {
  switch (event.data.action) {
    case 'open':
      document.getElementById('items').innerHTML = event.data.items
        .map((item) => `<li><button data-item="${item.sName}">${item.sName} (${item.iPrice})</button></li>`)
        .join('');
      document.body.hidden = false;
      break;
    case 'close':
      document.body.hidden = true;
      break;
  }
});

document.getElementById('items').addEventListener('click', (event) => {
  const item = event.target.dataset.item;
  if (item) post('buy', { item });
});

document.getElementById('close').addEventListener('click', () => post('close'));
