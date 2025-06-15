#!/bin/bash

GITHUB_REPO=${GITHUB_REPO:-"cestef/punch"}
SKIP_VERSION_CHECK=${SKIP_VERSION_CHECK:-false}
SPECIFIC_VERSION=${SPECIFIC_VERSION:-""}
NO_COLORS=${NO_COLORS:-false}

if [ "$NO_COLORS" = true ]; then
    C_INFO="" C_SUCCESS="" C_WARNING="" C_ERROR="" C_RESET=""
else
    C_INFO="\033[0;36m" C_SUCCESS="\033[0;32m" C_WARNING="\033[0;33m" 
    C_ERROR="\033[0;31m" C_RESET="\033[0m"
fi

msg() { echo -e "${1}${3:+[${3}] }${2}${C_RESET}"; }

msg "$C_INFO" "Installing punch..." "INFO"

# Fetch latest version if needed
if [ -z "$SPECIFIC_VERSION" ] && [ "$SKIP_VERSION_CHECK" != "true" ]; then
    msg "$C_INFO" "Fetching latest version information..." "INFO"
    LATEST_VERSION=$(curl -s "https://api.github.com/repos/$GITHUB_REPO/releases/latest" | 
                    grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    
    if [ -z "$LATEST_VERSION" ]; then
        msg "$C_ERROR" "Failed to fetch latest version." "ERROR"
        exit 1
    fi
    
    msg "$C_SUCCESS" "Latest version: $LATEST_VERSION" "INFO"
    VERSION_TO_INSTALL=$LATEST_VERSION
else
    VERSION_TO_INSTALL=$SPECIFIC_VERSION
    msg "$C_INFO" "Using specified version: ${VERSION_TO_INSTALL:-"latest"}" "INFO"
fi

msg "$C_INFO" "Downloading installer..." "INFO"
INSTALLER_URL="https://github.com/$GITHUB_REPO/releases/download/${VERSION_TO_INSTALL}/punch-installer.sh"
msg "$C_INFO" "From: $INSTALLER_URL" "INFO"

curl --proto '=https' --tlsv1.2 -LsSf "$INSTALLER_URL" | sh

if [ $? -eq 0 ]; then
    msg "$C_SUCCESS" "Installation completed successfully!" "SUCCESS"
else
    msg "$C_ERROR" "Installation failed." "ERROR"
    exit 1
fi
