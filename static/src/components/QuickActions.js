import React, { useState } from 'react';
import History from './History';
import Documentation from './Documentation';

const QuickActions = () => {
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [isDocumentationOpen, setIsDocumentationOpen] = useState(false);

  return (
    <>
      <div className="quick-actions">
        <button 
          className="quick-action-btn history-btn"
          onClick={() => setIsHistoryOpen(true)}
          title="Measurement History"
        >
          <span className="btn-text">HISTORY</span>
        </button>
        
        <button 
          className="quick-action-btn docs-btn"
          onClick={() => setIsDocumentationOpen(true)}
          title="Documentation"
        >
          <span className="btn-text">DOCS</span>
        </button>
      </div>

      <History
        isOpen={isHistoryOpen}
        onClose={() => setIsHistoryOpen(false)}
      />
      
      <Documentation
        isOpen={isDocumentationOpen}
        onClose={() => setIsDocumentationOpen(false)}
      />
    </>
  );
};

export default QuickActions; 