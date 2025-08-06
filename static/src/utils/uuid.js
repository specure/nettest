// Generate a random UUID
export const generateUUID = () => {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
    const r = Math.random() * 16 | 0;
    const v = c === 'x' ? r : (r & 0x3 | 0x8);
    return v.toString(16);
  });
};

// Get or create client UUID from localStorage
export const getClientUUID = () => {
  let clientUUID = localStorage.getItem('client_uuid');
  
  if (!clientUUID) {
    clientUUID = generateUUID();
    localStorage.setItem('client_uuid', clientUUID);
    console.log('Generated new client UUID:', clientUUID);
  }
  
  return clientUUID;
}; 