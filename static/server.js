const express = require('express');
const cors = require('cors');
const axios = require('axios');

const app = express();
const PORT = 3001;

// Middleware
app.use(cors());
app.use(express.json());

// Configuration
const CONTROL_SERVER_URL = 'https://api-beta.nettest.org';
const API_DEV_URL = 'https://api-beta.nettest.org';
const X_NETTEST_CLIENT = 'nt';

// Routes
app.get('/api/servers', async (req, res) => {
    try {
        const response = await axios.get(`${CONTROL_SERVER_URL}/measurementServer`, {
            headers: {
                'x-nettest-client': X_NETTEST_CLIENT,
                'Content-Type': 'application/json'
            }
        });
        res.json(response.data);
    } catch (error) {
        console.error('Error fetching servers:', error);
        res.status(500).json({ error: 'Failed to fetch servers' });
    }
});

// Proxy for all API requests to bypass CORS
app.all('/api/proxy/*', async (req, res) => {
    try {
        const targetPath = req.path.replace('/api/proxy', '');
        const targetUrl = `${API_DEV_URL}${targetPath}`;
        
        console.log('=== PROXY REQUEST ===');
        console.log(`Method: ${req.method}`);
        console.log(`Target URL: ${targetUrl}`);
        console.log('Original URL:', req.originalUrl);
        console.log('Request headers:', JSON.stringify(req.headers, null, 2));
        console.log('Request body:', JSON.stringify(req.body, null, 2));
        console.log('Request query:', JSON.stringify(req.query, null, 2));
        
        // Prepare headers for the target request
        const targetHeaders = {
            'Content-Type': 'application/json',
            'X-Nettest-Client': 'nt',
            'Accept': 'application/json'
        };
        
        // Add any additional headers from the original request
        if (req.headers.authorization) {
            targetHeaders.authorization = req.headers.authorization;
        }
        
        console.log('Target headers:', JSON.stringify(targetHeaders, null, 2));
        
        const response = await axios({
            method: req.method,
            url: targetUrl,
            headers: targetHeaders,
            data: req.body,
            params: req.query,
            timeout: 30000 // 30 second timeout
        });
        
        console.log('=== PROXY RESPONSE ===');
        console.log(`Status: ${response.status}`);
        console.log('Response headers:', JSON.stringify(response.headers, null, 2));
        console.log('Response data:', JSON.stringify(response.data, null, 2));
        
        res.status(response.status).json(response.data);
    } catch (error) {
        console.error('=== PROXY ERROR ===');
        console.error('Error message:', error.message);
        console.error('Error code:', error.code);
        console.error('Error response status:', error.response?.status);
        console.error('Error response data:', error.response?.data);
        console.error('Error response headers:', error.response?.headers);
        
        res.status(error.response?.status || 500).json({ 
            error: 'Proxy request failed',
            details: error.response?.data || error.message,
            status: error.response?.status || 500
        });
    }
});

app.listen(PORT, () => {
    console.log(`Server running on port ${PORT}`);
    console.log(`API available at http://localhost:${PORT}`);
    console.log(`Proxy available at http://localhost:${PORT}/api/proxy/*`);
}); 