uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c2 = float4(0.0, 2.0, 0.239999995, 4.0);
static const float4 c3 = float4(6.0, 8.0, 0.400000006, 0.159999996);
static const float4 c4 = float4(0.140000001, 0.059999999, 1.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r5 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r6 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r7 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r8 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r9 = float4(0.0, 0.0, 0.0, 0.0);

    r0.x = (-(input.t0.x) + c0.z);
    r0.y = (-(input.t0.y) + c0.w);
    r0.z = (dot(r0.xy, r0.xy) + c2.x);
    r0.z = rsqrt(r0.z);
    r0.xy = (r0.xy * r0.zz);
    r0.z = (1.0 / r0.z);
    r0.xy = (r0.xy * c0.xx);
    r0.z = (r0.z * c1.x);
    r0.z = (r0.z * c1.y);
    r0.xy = (r0.xy * r0.zz);
    r1.xy = (r0.xy * c2.yy + input.t0.xy);
    r2.xy = (r0.xy * c2.ww + input.t0.xy);
    r3.xy = (r0.xy * c3.xx + input.t0.xy);
    r0.xy = (r0.xy * c3.yy + input.t0.xy);
    r4 = tex2D(s0, r1.xy);
    r1 = tex2D(s1, r1.xy);
    r5 = tex2D(s0, input.t0.xy);
    r6 = tex2D(s0, r2.xy);
    r2 = tex2D(s1, r2.xy);
    r7 = tex2D(s0, r3.xy);
    r3 = tex2D(s1, r3.xy);
    r8 = tex2D(s0, r0.xy);
    r0 = tex2D(s1, r0.xy);
    r9 = tex2D(s1, input.t0.xy);
    r4 = (r4 * c2.zzzz);
    r4 = (r5 * c3.zzzz + r4);
    r4 = (r6 * c3.wwww + r4);
    r4 = (r7 * c4.xxxx + r4);
    r4 = (r8 * c4.yyyy + r4);
    r5.x = (-(input.v0.w) + c4.z);
    r4 = (r4 * r5.xxxx);
    r1 = (r1 * c2.zzzz);
    r1 = (r9 * c3.zzzz + r1);
    r1 = (r2 * c3.wwww + r1);
    r1 = (r3 * c4.xxxx + r1);
    r0 = (r0 * c4.yyyy + r1);
    r0 = (input.v0.wwww * r0 + r4);
    output.oC0 = r0;
    return output;
}
