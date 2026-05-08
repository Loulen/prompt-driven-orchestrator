// confirm-modal.jsx — destructive confirm modal

function ConfirmDelete({ open, onClose, name, kind = 'pipeline', detail }) {
  if (!open) return null;
  return (
    <div className="modal-bg confirm-modal" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="cm-icon"><Ic.Trash/></div>
        <h3>Delete this {kind}?</h3>
        <p>
          <span className="cm-name">{name}</span>{' '}
          {detail || `will be permanently deleted along with its run history. This cannot be undone.`}
        </p>
        <div className="cm-foot">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn warn" style={{background:'rgba(239,68,68,0.12)', color:'#fca5a5', borderColor:'rgba(239,68,68,0.32)'}}>
            <Ic.Trash/> Delete {kind}
          </button>
        </div>
      </div>
    </div>
  );
}

window.ConfirmDelete = ConfirmDelete;
