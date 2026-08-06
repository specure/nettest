# Deploy to GitHub Pages

## Automatic deploy

The app is deployed to GitHub Pages automatically on every push to the `main`
branch.

### App URLs
- **Production**: https://specure.github.io/nettest
- **Development**: http://localhost:3000

## Manual deploy

If you need to deploy manually:

```bash
# Install dependencies
cd static
npm install

# Build the app
npm run build

# Deploy to GitHub Pages
npm run deploy
```

## GitHub Pages settings

1. Go to the repository Settings
2. Find the "Pages" section
3. Source: "Deploy from a branch"
4. Branch: "gh-pages"
5. Folder: "/ (root)"

## File structure

```
├── .github/workflows/deploy.yml  # GitHub Actions
├── static/
│   ├── package.json              # gh-pages settings
│   ├── public/
│   │   └── _redirects            # For React Router
│   └── src/                      # Source code
└── DEPLOY.md                     # This guide
```

## Verifying the deploy

After deploying, check:
1. https://specure.github.io/nettest — does the app load
2. Server map — are the servers shown
3. Measurements — do the tests work
4. Documentation — does the modal open

## Troubleshooting

### Issue: 404 on GitHub Pages
**Fix**: Check the `_redirects` file in `static/public/`

### Issue: CORS errors
**Fix**: Make sure the API endpoints are configured correctly

### Issue: Map does not load
**Fix**: Check that Leaflet loads correctly
