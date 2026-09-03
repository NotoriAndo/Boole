# Operational key custody plan v1

Status: **VALIDATION IMPLEMENTED / REAL CUSTODIANS AND DEVICES NOT YET SUPPLIED / NO KEY OR AUTHORITY CREATED**.

This plan is the last public-metadata gate before a real release-key ceremony may be prepared. It
does not contain private keys and does not authorize key generation, signing, publication, release,
production or network activation. `boole product verify-operational-custody-plan` only reads one
canonical JSON file and returns its SHA-256 plus a readiness result.

The boundary follows the repository's existing two-of-three recovery policy. Product and guest
release roles use separate custodians and dedicated signing devices. Recovery A, B and C use three
different custodians, devices and physical site identifiers, and their media stays offline. The
bootstrap package and recovery-root digest use different HTTPS hosts and different administrative
control domains; changing labels while one account controls both is rejected.

These requirements are consistent with NIST's guidance to protect key material and its metadata
through the full key-management lifecycle and with TUF's requirement that root private keys be kept
offline and protected by a threshold:

- <https://csrc.nist.gov/pubs/sp/800/57/pt1/r5/final>
- <https://theupdateframework.github.io/specification/latest/#root-role>

## Operator input

Copy the shape below and replace every angle-bracket placeholder. Angle brackets are intentionally
invalid identifiers, so the example cannot accidentally pass as an approved operational plan.
Use opaque operator/device/site references rather than names, email addresses, serial numbers,
passwords, seed phrases, private-key paths or private-key material.

```json
{
  "approval": {
    "approvalId": "<approval-id>",
    "operatorId": "<operator-id>",
    "scope": "ceremony-preparation-only"
  },
  "assignments": [
    {
      "custodianId": "<product-custodian>",
      "custodyClass": "online-signing",
      "deviceClass": "dedicated-online-signer",
      "deviceId": "<product-device>",
      "role": "product-release",
      "siteId": "<online-site-a>"
    },
    {
      "custodianId": "<guest-custodian>",
      "custodyClass": "online-signing",
      "deviceClass": "dedicated-online-signer",
      "deviceId": "<guest-device>",
      "role": "guest-release",
      "siteId": "<online-site-b>"
    },
    {
      "custodianId": "<recovery-custodian-a>",
      "custodyClass": "offline-recovery",
      "deviceClass": "offline-removable-media",
      "deviceId": "<recovery-device-a>",
      "role": "recovery-a",
      "siteId": "<recovery-site-a>"
    },
    {
      "custodianId": "<recovery-custodian-b>",
      "custodyClass": "offline-recovery",
      "deviceClass": "offline-removable-media",
      "deviceId": "<recovery-device-b>",
      "role": "recovery-b",
      "siteId": "<recovery-site-b>"
    },
    {
      "custodianId": "<recovery-custodian-c>",
      "custodyClass": "offline-recovery",
      "deviceClass": "offline-removable-media",
      "deviceId": "<recovery-device-c>",
      "role": "recovery-c",
      "siteId": "<recovery-site-c>"
    }
  ],
  "controls": {
    "ceremonyNeedsTwoRecoveryCustodians": true,
    "privateKeysForbiddenFromRepository": true,
    "productionActivationExcluded": true,
    "recoveryDevicesRemainOffline": true
  },
  "environment": "operational-production-readiness",
  "planId": "<plan-id>",
  "publication": {
    "bootstrap": {
      "channelId": "<bootstrap-channel>",
      "controlDomainId": "<bootstrap-administrator>",
      "httpsUrl": "https://<bootstrap-host>/<immutable-path>"
    },
    "recoveryRootPin": {
      "channelId": "<root-pin-channel>",
      "controlDomainId": "<independent-administrator>",
      "httpsUrl": "https://<different-host>/<root-pin-path>"
    },
    "rootPinFormat": "sha256-lowercase-hex",
    "rootPinMustPrecedeAdoption": true
  },
  "schema": "boole.operational-key-custody-plan.v1"
}
```

Canonicalize the completed JSON using the same canonical JSON encoder as the other release
authority documents, then run:

```text
boole product verify-operational-custody-plan --plan operational-custody-plan.json
```

A GREEN result means only that the public plan is internally complete and separated. It is not
proof that the named custodians, devices, sites or channel administrators exist or are independent.
The real ceremony must later bind this exact `planSha256` before any operational key is generated.
