import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import type { StorageSettingsDTO } from '@meeting-notes/contracts'
import { StorageSettings } from '../components/admin/storage-panel'
import type { StorageDraft } from './instance-settings'

const saved: StorageSettingsDTO = { backend: 'local', source: 'default', endpoint: null, region: 'us-east-1', bucket: null, prefix: '', forcePathStyle: false, accessKeyId: null, secretAccessKeySet: false }
const draft: StorageDraft = { backend: 'local', endpoint: '', region: 'us-east-1', bucket: '', prefix: '', forcePathStyle: false, accessKeyId: '', secretAccessKey: '', removeSecret: false }
const render = (backend: 'local' | 's3') => renderToStaticMarkup(<StorageSettings draft={{ ...draft, backend }} saved={saved} change={() => {}} test={async () => ({ ok: true, message: '' }) as never} />)

describe('storage settings', () => {
  test('local disk asks for no bucket, and can still be tested', () => {
    const markup = render('local')
    expect(markup).not.toContain('storage-bucket')
    expect(markup).not.toContain('Secret access key')
    expect(markup).toContain('Test connection')
    expect(markup).toContain('Checks that the server can write to its disk.')
  })
  test('S3 shows the bucket fields with the test under them', () => {
    const markup = render('s3')
    for (const label of ['Endpoint', 'Region', 'Bucket', 'Access key ID', 'Secret access key', 'Path-style addresses']) expect(markup).toContain(label)
    expect(markup.indexOf('Test connection')).toBeGreaterThan(markup.indexOf('Secret access key'))
    expect(markup.match(/Test connection/g)?.length).toBe(1)
  })
  test('drawn again (as after every Save), the Bucket section does not rise in again', () => {
    expect(render('s3')).toContain('class="model-section storage-bucket"')
    expect(render('s3')).not.toContain('just-chosen')
  })
})
