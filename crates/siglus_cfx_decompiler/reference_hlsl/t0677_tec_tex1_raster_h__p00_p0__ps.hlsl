uniform sampler2D s0;

static const float4 c7 = float4(0.0, 1.0, 3.140000105, 0.0);
static const float4 c8 = float4(0.159154937, 0.5, 6.283185482, -3.141592741);
static const float4 c9 = float4(-0.00000155, -0.000021701, 0.002604167, 0.000260417);
static const float4 c10 = float4(-0.020833334, -0.125, 1.0, 0.5);

struct PS_INPUT {
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

    r0.w = (input.t0.y * c6.x);
    r0.x = frac(r0.w);
    r0.y = (r0.w + -(r0.x));
    r0.x = (-(r0.x) >= 0 ? c7.y : c7.x);
    r0.z = (r0.w >= 0 ? c7.y : c7.x);
    r0.x = (r0.z * r0.x + r0.y);
    r0.x = (r0.x + -(c0.x));
    r0.x = (r0.x * c1.x);
    r0.x = (r0.x * c6.y);
    r0.z = c7.z;
    r0.x = (r0.x * r0.z + c2.x);
    r0.x = (r0.x * c8.x + c8.y);
    r0.x = frac(r0.x);
    r0.x = (r0.x * c8.z + c8.w);
    r1.y = sin(r0.x);
    r0.x = (r1.y * c5.x + input.t0.x);
    r0.z = (-(r0.x) + c7.y);
    r0.z = (r0.z >= 0 ? c7.y : c7.x);
    r0.w = (r0.x >= 0 ? c7.y : c7.x);
    r0.z = (r0.z + r0.w);
    r0.z = (-(r0.z) >= 0 ? c7.y : c7.x);
    r0.w = (input.t0.y >= 0 ? c7.y : c7.x);
    r0.z = (r0.z + r0.w);
    r0.z = (-(r0.z) >= 0 ? c7.y : c7.x);
    r0.w = (-(input.t0.y) + c7.y);
    r0.w = (r0.w >= 0 ? c7.y : c7.x);
    r0.z = (r0.z + r0.w);
    r0.y = input.t0.y;
    r1 = tex2D(s0, r0.xy);
    r0 = (-(r0.zzzz) >= 0 ? c7.xxxx : r1);
    r0.w = (r0.w * c6.w);
    output.oC0 = r0;
    return output;
}
