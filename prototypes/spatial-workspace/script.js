document.addEventListener('DOMContentLoaded', () => {
  const appContainer = document.getElementById('app');
  const layoutBtns = document.querySelectorAll('.layout-btn');

  layoutBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      // Remove active class from all buttons
      layoutBtns.forEach(b => b.classList.remove('active'));
      // Add active class to clicked button
      btn.classList.add('active');
      
      // Update layout class on container
      const newLayout = btn.getAttribute('data-layout');
      
      // Remove existing layout classes
      appContainer.classList.remove('layout-focus', 'layout-pairing', 'layout-review', 'layout-pipeline');
      
      // Add new layout class
      appContainer.classList.add(newLayout);
    });
  });

  // Action card toggle
  const toggleBtn = document.querySelector('.toggle-btn');
  const actionCard = document.querySelector('.action-card');
  const caretIcon = toggleBtn.querySelector('i');

  toggleBtn.addEventListener('click', () => {
    actionCard.classList.toggle('expanded');
    if (actionCard.classList.contains('expanded')) {
      caretIcon.classList.replace('ph-caret-down', 'ph-caret-up');
    } else {
      caretIcon.classList.replace('ph-caret-up', 'ph-caret-down');
    }
  });
});
