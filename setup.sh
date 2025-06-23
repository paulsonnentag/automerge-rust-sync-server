#!/bin/bash

# Simple setup script for Automerge Rust Sync Server services

set -e

echo "Setting up Automerge Rust Sync Server services..."

# Stop and disable existing services if they exist
echo "Stopping existing services..."
sudo systemctl stop sync-server-dev.service 2>/dev/null || true
sudo systemctl stop sync-server-prod.service 2>/dev/null || true

sudo systemctl disable sync-server-dev.service 2>/dev/null || true
sudo systemctl disable sync-server-prod.service 2>/dev/null || true

# Remove old service files
echo "Removing old service files..."
sudo rm -f /etc/systemd/system/sync-server-dev.service
sudo rm -f /etc/systemd/system/sync-server-prod.service

# Copy service files to systemd directory
echo "Installing new service files..."
sudo cp sync-server-dev.service /etc/systemd/system/
sudo cp sync-server-prod.service /etc/systemd/system/

# Reload systemd
sudo systemctl daemon-reload

# Start the services
echo "Starting services..."
sudo systemctl start sync-server-dev.service
sudo systemctl start sync-server-prod.service

# Enable services to start on boot
sudo systemctl enable sync-server-dev.service
sudo systemctl enable sync-server-prod.service

echo "Services started and enabled!"
echo ""
echo "Development server: http://localhost:81 (port 8081)"
echo "Production server:  http://localhost:80 (port 8080)"
echo ""
echo "Check status with:"
echo "  sudo systemctl status sync-server-dev.service"
echo "  sudo systemctl status sync-server-prod.service"
