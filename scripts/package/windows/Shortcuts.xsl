<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:wix="http://schemas.microsoft.com/wix/2006/wi" exclude-result-prefixes="wix">
  <xsl:output method="xml" indent="yes" />
  <xsl:template match="@*|node()">
    <xsl:copy><xsl:apply-templates select="@*|node()" /></xsl:copy>
  </xsl:template>
  <!-- An advertised shortcut belongs to the executable's file component,
       avoiding per-user registry key paths in this per-machine package. -->
  <xsl:template match="wix:File[@Source='$(var.SourceDir)\Sidedoor.exe']">
    <xsl:copy>
      <xsl:apply-templates select="@*|node()" />
      <Shortcut xmlns="http://schemas.microsoft.com/wix/2006/wi" Id="SidedoorShortcut"
                Directory="AppMenuFolder" Name="Sidedoor" WorkingDirectory="INSTALLFOLDER"
                Advertise="yes" />
    </xsl:copy>
  </xsl:template>
  <xsl:template match="wix:Component[wix:File[@Source='$(var.SourceDir)\Sidedoor.exe']]">
    <xsl:copy>
      <xsl:apply-templates select="@*|node()" />
      <RemoveFolder xmlns="http://schemas.microsoft.com/wix/2006/wi" Id="RemoveAppMenuFolder"
                    Directory="AppMenuFolder" On="uninstall" />
    </xsl:copy>
  </xsl:template>
</xsl:stylesheet>
